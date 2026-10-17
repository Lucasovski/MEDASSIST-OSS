//! MedAssist OSS CLI - Command-line administration tool.
//!
//! Provides database migrations, admin user creation, and maintenance utilities.
//!
//! License: GPL-3.0-or-later

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
    Algorithm, Argon2, Params, Version,
};
use clap::{Parser, Subcommand};
use dotenvy::dotenv;
use medassist_infrastructure::db::Db;
use sqlx::PgPool;
use std::path::Path;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn argon2id() -> Argon2<'static> {
    Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::default())
}

/// MedAssist OSS - Command Line Interface
#[derive(Debug, Parser)]
#[command(name = "medassist")]
#[command(about = "MedAssist OSS administration tool", long_about = None)]
struct Cli {
    /// Log verbosely
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

/// Available commands
#[derive(Debug, Subcommand)]
enum Commands {
    /// Apply database migrations
    Migrate,

    /// Create a new database and apply migrations
    Setup,

    /// Create an administrator user
    CreateAdmin {
        /// Organization name
        #[arg(short, long)]
        org_name: String,
        /// Administrator email
        #[arg(short, long)]
        email: String,
        /// Administrator password (will be hashed)
        #[arg(short, long)]
        password: String,
        /// Administrator display name
        #[arg(short, long)]
        display_name: String,
    },

    /// Reset a user's password
    ResetPassword {
        /// User email
        #[arg(short, long)]
        email: String,
        /// New password
        #[arg(short, long)]
        password: String,
    },

    /// Verify database connection
    CheckDb,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load environment variables
    dotenv().ok();

    // Initialize logging
    let subscriber = tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer().json());
    subscriber.init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Migrate => {
            let db = connect_db().await?;
            run_migrations(&db).await?;
            println!("\u{2713} Migrations applied successfully");
        }
        Commands::Setup => {
            create_database().await?;
            let db = connect_db().await?;
            run_migrations(&db).await?;
            println!("\u{2713} Database setup complete");
        }
        Commands::CreateAdmin {
            org_name,
            email,
            password,
            display_name,
        } => {
            let db = connect_db().await?;
            create_admin(&db, &org_name, &email, &password, &display_name).await?;
            println!("\u{2713} Administrator created successfully");
        }
        Commands::ResetPassword { email, password } => {
            let db = connect_db().await?;
            reset_password(&db, &email, &password).await?;
            println!("\u{2713} Password reset successfully");
        }
        Commands::CheckDb => {
            let db = connect_db().await?;
            sqlx::query("SELECT 1").execute(db.pool()).await?;
            println!("\u{2713} Database connection OK");
        }
    }

    Ok(())
}

/// Connect to the database
async fn connect_db() -> anyhow::Result<Db> {
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/medassist".to_string());
    Db::connect(&db_url, 5)
        .await
        .map_err(|e| anyhow::anyhow!("Database connection failed: {}", e))
}

/// Create the database if it doesn't exist
async fn create_database() -> anyhow::Result<()> {
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/medassist".to_string());
    let db_name = extract_db_name(&db_url);
    let admin_url = format_admin_url(&db_url);

    // Check if database exists
    let admin_pool = PgPool::connect(&admin_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to admin database: {}", e))?;

    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(&db_name)
            .fetch_one(&admin_pool)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to check database existence: {}", e))?;

    if !exists {
        sqlx::query(&format!("CREATE DATABASE \"{}\"", db_name))
            .execute(&admin_pool)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create database: {}", e))?;
        println!("\u{2713} Database '{}' created", db_name);
    } else {
        println!("\u{2713} Database '{}' already exists", db_name);
    }

    Ok(())
}

/// Run database migrations
async fn run_migrations(db: &Db) -> anyhow::Result<()> {
    let migration_path = Path::new("migrations");
    if !migration_path.exists() {
        anyhow::bail!(
            "Migrations directory not found at: {}",
            migration_path.display()
        );
    }

    db.migrate(migration_path)
        .await
        .map_err(|e| anyhow::anyhow!("Migration failed: {}", e))?;

    Ok(())
}

/// Create an administrator user with organization
async fn create_admin(
    db: &Db,
    org_name: &str,
    email: &str,
    password: &str,
    display_name: &str,
) -> anyhow::Result<()> {
    use uuid::Uuid;

    // Create organization
    let org_id = Uuid::new_v4();
    sqlx::query("INSERT INTO organisations (id, name) VALUES ($1, $2)")
        .bind(org_id)
        .bind(org_name)
        .execute(db.pool())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create organization: {}", e))?;

    // Hash password
    let salt = SaltString::generate(&mut OsRng);
    let password_hash = hash_password(password, &salt)?;

    // Create user
    let user_id = Uuid::new_v4();
    let roles = vec!["admin", "clinician"];
    sqlx::query(
        r#"
        INSERT INTO users 
        (id, org_id, email, password_hash, display_name, roles, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, now(), now())
        "#,
    )
    .bind(user_id)
    .bind(org_id)
    .bind(email)
    .bind(password_hash)
    .bind(display_name)
    .bind(roles)
    .execute(db.pool())
    .await
    .map_err(|e| anyhow::anyhow!("Failed to create user: {}", e))?;

    println!("  Organization ID: {}", org_id);
    println!("  User ID: {}", user_id);
    println!("  Email: {}", email);
    println!("  Display Name: {}", display_name);

    Ok(())
}

/// Reset a user's password
async fn reset_password(db: &Db, email: &str, password: &str) -> anyhow::Result<()> {
    // Hash new password
    let salt = SaltString::generate(&mut OsRng);
    let password_hash = hash_password(password, &salt)?;

    // Update password
    let result = sqlx::query(
        "UPDATE users SET password_hash = $1, failed_logins = 0, locked_until = NULL WHERE email = $2",
    )
    .bind(&password_hash)
    .bind(email)
    .execute(db.pool())
    .await
    .map_err(|e| anyhow::anyhow!("Failed to update password: {}", e))?;
    let rows_affected = result.rows_affected();

    if rows_affected == 0 {
        anyhow::bail!("User with email '{}' not found", email);
    }

    println!("\u{2713} Password reset for: {}", email);

    Ok(())
}

/// Hash a password using Argon2id
fn hash_password(password: &str, salt: &SaltString) -> Result<String, anyhow::Error> {
    let hash = argon2id()
        .hash_password(password.as_bytes(), salt)
        .map_err(|e| anyhow::anyhow!("Password hashing failed: {:?}", e))?;
    Ok(hash.to_string())
}

/// Extract database name from connection URL
fn extract_db_name(db_url: &str) -> String {
    // Parse URL to extract database name
    // Format: postgres://user:pass@host:port/dbname
    if let Some(slash_pos) = db_url.rfind('/') {
        let after_slash = &db_url[slash_pos + 1..];
        // Handle query parameters
        if let Some(q_pos) = after_slash.find('?') {
            &after_slash[..q_pos]
        } else {
            after_slash
        }
    } else {
        db_url
    }
    .to_string()
}

/// Format admin URL (without database name) for database creation
fn format_admin_url(db_url: &str) -> String {
    // Replace dbname with 'postgres' (default admin database)
    if let Some(slash_pos) = db_url.rfind('/') {
        format!("{}/postgres", &db_url[..slash_pos])
    } else {
        format!("{}/postgres", db_url)
    }
}
