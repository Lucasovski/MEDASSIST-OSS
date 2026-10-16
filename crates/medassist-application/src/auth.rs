//! Authentication use-case: Argon2id password hashing, constant-time verify,
//! lockout after repeated failures, opaque session tokens (only their SHA-256
//! hash is stored). The real HTTP wiring lives in the API crate.

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Algorithm, Argon2, Params, Version,
};
use chrono::{DateTime, Duration};
use sha2::{Digest as _, Sha256};
use uuid::Uuid as Guid;

use crate::errors::{Actor, AppError};
use crate::ports::{
    AuditEvent, AuditOutcome, AuditSink, NewUserRecord, SessionRecord, SessionRepository,
    UserRepository,
};

pub const MAX_FAILED_LOGINS: i32 = 5;
pub const LOCKOUT_MINUTES: i64 = 15;
pub const SESSION_TTL_DAYS: i64 = 7;
/// Minimum accepted password length (NIST SP 800-63B guidance: >= 8).
pub const MIN_PASSWORD_LEN: usize = 8;

pub fn argon2id() -> Argon2<'static> {
    // m=19MiB, t=2, p=1 — OWASP-ish parameters for Argon2id.
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::default())
}

pub fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    argon2id()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Unprocessable(format!("password hashing failed: {e}")))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => argon2id()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

pub fn validate_password_strength(password: &str) -> Result<(), AppError> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(AppError::Unprocessable(format!(
            "password must have at least {MIN_PASSWORD_LEN} characters"
        )));
    }
    if password.len() > 256 {
        return Err(AppError::Unprocessable("password too long".into()));
    }
    Ok(())
}

/// Generate a cryptographically random opaque session token.
pub fn generate_session_token() -> String {
    use rand::Rng;
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill(&mut bytes);
    hex_encode(&bytes)
}

/// SHA-256 hex of the token — sessions are looked up by hash so a database
/// leak cannot be replayed directly.
pub fn hash_token(token: &str) -> String {
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    hex_encode(&h.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

pub struct AuthService<U: UserRepository, S: SessionRepository, A: AuditSink> {
    pub users: U,
    pub sessions: S,
    pub audit: A,
}

impl<U: UserRepository, S: SessionRepository, A: AuditSink> AuthService<U, S, A> {
    pub fn new(users: U, sessions: S, audit: A) -> Self {
        Self {
            users,
            sessions,
            audit,
        }
    }

    pub async fn register_first_admin(
        &self,
        org_id: Guid,
        email: &str,
        password: &str,
    ) -> Result<Guid, AppError> {
        validate_password_strength(password)?;
        let email = normalise_email(email)?;
        if self.users.find_by_email(&email).await?.is_some() {
            return Err(AppError::Conflict("email already registered".into()));
        }
        let hash = hash_password(password)?;
        self.users
            .insert(&NewUserRecord {
                org_id,
                email,
                password_hash: hash,
                roles: vec!["admin".into(), "practitioner".into()],
            })
            .await
    }

    /// Login returns `(session_token, actor)`. On bad credentials it performs
    /// a dummy hash to reduce user-enumeration timing differences.
    pub async fn login(
        &self,
        email: &str,
        password: &str,
        now: DateTime<chrono::Utc>,
    ) -> Result<(String, Actor), AppError> {
        let email = normalise_email(email)?;
        let user = match self.users.find_by_email(&email).await? {
            Some(u) => u,
            None => {
                let _ = verify_password(password, "$argon2id$v=19$m=19456,t=2,p=1$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
                self.audit(None, "auth.login", AuditOutcome::Denied).await;
                return Err(AppError::Unauthenticated);
            }
        };

        if let Some(until) = user.locked_until {
            if until > now {
                self.audit(Some(user.id), "auth.login.locked", AuditOutcome::Denied)
                    .await;
                return Err(AppError::Unauthenticated);
            }
        }

        if !verify_password(password, &user.password_hash) {
            self.users.register_login_failure(user.id).await?;
            let fails = user.failed_logins + 1;
            if fails >= MAX_FAILED_LOGINS {
                self.users
                    .lock_until(user.id, now + Duration::minutes(LOCKOUT_MINUTES))
                    .await?;
                self.audit(
                    Some(user.id),
                    "auth.login.lockout_triggered",
                    AuditOutcome::Failure,
                )
                .await;
            }
            self.audit(
                Some(user.id),
                "auth.login.bad_password",
                AuditOutcome::Denied,
            )
            .await;
            return Err(AppError::Unauthenticated);
        }

        self.users.register_login_success(user.id, now).await?;
        let token = generate_session_token();
        let session = SessionRecord {
            token_hash: hash_token(&token),
            user_id: user.id,
            org_id: user.org_id,
            roles: user.roles.clone(),
            expires_at: now + Duration::days(SESSION_TTL_DAYS),
            revoked_at: None,
        };
        self.sessions.insert(&session).await?;
        self.audit(Some(user.id), "auth.login", AuditOutcome::Success)
            .await;
        Ok((
            token,
            Actor {
                user_id: user.id,
                org_id: user.org_id,
                roles: user.roles,
            },
        ))
    }

    pub async fn resolve_session(
        &self,
        token: &str,
        now: DateTime<chrono::Utc>,
    ) -> Result<Actor, AppError> {
        let hash = hash_token(token);
        match self.sessions.find_by_token_hash(&hash).await? {
            Some(s) if s.revoked_at.is_none() && s.expires_at > now => Ok(Actor {
                user_id: s.user_id,
                org_id: s.org_id,
                roles: s.roles,
            }),
            _ => Err(AppError::Unauthenticated),
        }
    }

    pub async fn logout(&self, token: &str, now: DateTime<chrono::Utc>) -> Result<(), AppError> {
        let hash = hash_token(token);
        self.sessions.revoke(&hash, now).await
    }

    async fn audit(&self, user_id: Option<Guid>, action: &str, outcome: AuditOutcome) {
        self.audit
            .record(AuditEvent {
                time: chrono::Utc::now(),
                actor_user_id: user_id,
                org_id: None,
                action: action.into(),
                resource_type: "auth".into(),
                resource_id: None,
                outcome,
                metadata: serde_json::json!({}),
            })
            .await;
    }
}

pub fn normalise_email(email: &str) -> Result<String, AppError> {
    let e = email.trim().to_lowercase();
    let ok = e.len() <= 254 && e.contains('@') && e.rfind('.') > Some(e.find('@').unwrap_or(0));
    if !ok {
        return Err(AppError::Unprocessable("invalid email address".into()));
    }
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemUsers {
        by_email: Mutex<HashMap<String, crate::ports::UserRecord>>,
    }

    #[async_trait::async_trait]
    impl UserRepository for MemUsers {
        async fn find_by_email(
            &self,
            email: &str,
        ) -> Result<Option<crate::ports::UserRecord>, AppError> {
            Ok(self.by_email.lock().unwrap().get(email).cloned())
        }
        async fn insert(&self, u: &NewUserRecord) -> Result<Guid, AppError> {
            let id = Guid::new_v4();
            self.by_email.lock().unwrap().insert(
                u.email.clone(),
                crate::ports::UserRecord {
                    id,
                    org_id: u.org_id,
                    email: u.email.clone(),
                    password_hash: u.password_hash.clone(),
                    roles: u.roles.clone(),
                    failed_logins: 0,
                    locked_until: None,
                },
            );
            Ok(id)
        }
        async fn update_password_hash(&self, _id: Guid, _hash: String) -> Result<(), AppError> {
            Ok(())
        }
        async fn register_login_success(
            &self,
            _id: Guid,
            _now: DateTime<chrono::Utc>,
        ) -> Result<(), AppError> {
            Ok(())
        }
        async fn register_login_failure(&self, id: Guid) -> Result<(), AppError> {
            let mut g = self.by_email.lock().unwrap();
            if let Some(u) = g.values_mut().find(|u| u.id == id) {
                u.failed_logins += 1;
            }
            Ok(())
        }
        async fn lock_until(&self, id: Guid, until: DateTime<chrono::Utc>) -> Result<(), AppError> {
            let mut g = self.by_email.lock().unwrap();
            if let Some(u) = g.values_mut().find(|u| u.id == id) {
                u.locked_until = Some(until);
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemSessions(Mutex<HashMap<String, SessionRecord>>);

    #[async_trait::async_trait]
    impl SessionRepository for MemSessions {
        async fn insert(&self, s: &SessionRecord) -> Result<(), AppError> {
            self.0
                .lock()
                .unwrap()
                .insert(s.token_hash.clone(), s.clone());
            Ok(())
        }
        async fn find_by_token_hash(&self, h: &str) -> Result<Option<SessionRecord>, AppError> {
            Ok(self.0.lock().unwrap().get(h).cloned())
        }
        async fn revoke(&self, h: &str, now: DateTime<chrono::Utc>) -> Result<(), AppError> {
            if let Some(s) = self.0.lock().unwrap().get_mut(h) {
                s.revoked_at = Some(now);
            }
            Ok(())
        }
        async fn revoke_all_for_user(
            &self,
            _u: Guid,
            _now: DateTime<chrono::Utc>,
        ) -> Result<(), AppError> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemAudit(Mutex<Vec<AuditEvent>>);
    #[async_trait::async_trait]
    impl AuditSink for MemAudit {
        async fn record(&self, ev: AuditEvent) {
            self.0.lock().unwrap().push(ev);
        }
    }

    fn svc() -> AuthService<MemUsers, MemSessions, MemAudit> {
        AuthService::new(
            MemUsers::default(),
            MemSessions::default(),
            MemAudit::default(),
        )
    }

    #[test]
    fn argon2id_hash_verifies_and_rejects_wrong_password() {
        let h = hash_password("correct horse battery staple").unwrap();
        assert!(h.starts_with("$argon2id$"), "must be argon2id, got {h}");
        assert!(verify_password("correct horse battery staple", &h));
        assert!(!verify_password("wrong", &h));
    }

    #[test]
    fn weak_passwords_rejected() {
        assert!(validate_password_strength("short1!").is_err());
        assert!(validate_password_strength("longenough1").is_ok());
    }

    #[test]
    fn email_normalisation_and_validation() {
        assert_eq!(
            normalise_email("  Ana.Silva@Example.COM ").unwrap(),
            "ana.silva@example.com"
        );
        assert!(normalise_email("not-an-email").is_err());
        assert!(normalise_email("a@b").is_err());
    }

    #[tokio::test]
    async fn login_creates_resolvable_session_and_logout_revokes_it() {
        let s = svc();
        let org = Guid::new_v4();
        s.register_first_admin(org, "admin@clinic.test", "sup3r-s3cret-pw")
            .await
            .unwrap();
        let (token, actor) = s
            .login("ADMIN@clinic.test", "sup3r-s3cret-pw", chrono::Utc::now())
            .await
            .unwrap();
        assert_eq!(actor.org_id, org);
        let resolved = s.resolve_session(&token, chrono::Utc::now()).await.unwrap();
        assert_eq!(resolved.user_id, actor.user_id);
        s.logout(&token, chrono::Utc::now()).await.unwrap();
        assert!(matches!(
            s.resolve_session(&token, chrono::Utc::now()).await,
            Err(AppError::Unauthenticated)
        ));
    }

    #[tokio::test]
    async fn lockout_after_max_failures() {
        let s = svc();
        let org = Guid::new_v4();
        s.register_first_admin(org, "u@clinic.test", "good-password-1")
            .await
            .unwrap();
        for _ in 0..MAX_FAILED_LOGINS {
            assert!(s
                .login("u@clinic.test", "bad-password-x", chrono::Utc::now())
                .await
                .is_err());
        }
        // Even the correct password is refused while locked.
        let later = chrono::Utc::now() + Duration::seconds(1);
        let err = s
            .login("u@clinic.test", "good-password-1", later)
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Unauthenticated));
        let events = s.audit.0.lock().unwrap();
        assert!(events
            .iter()
            .any(|e| e.action == "auth.login.lockout_triggered"));
    }

    #[tokio::test]
    async fn expired_session_is_rejected() {
        let s = svc();
        let org = Guid::new_v4();
        s.register_first_admin(org, "e@clinic.test", "another-good-pw")
            .await
            .unwrap();
        let (token, _) = s
            .login("e@clinic.test", "another-good-pw", chrono::Utc::now())
            .await
            .unwrap();
        let far_future = chrono::Utc::now() + Duration::days(SESSION_TTL_DAYS + 1);
        assert!(matches!(
            s.resolve_session(&token, far_future).await,
            Err(AppError::Unauthenticated)
        ));
    }
}
