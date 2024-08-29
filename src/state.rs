use std::{env, fmt::Display};

use crate::{db::*, serenity};
use log::{debug, error, warn};
use reqwest::StatusCode;
use serde::Serialize;
use serenity::all::{GuildId, Member};

pub struct BotState {
    pub http_client: reqwest::Client,
    pub base_url: String,
    pub server_password: String,
    pub database: sqlx::SqlitePool,
    pub guild_id: GuildId,
}

#[derive(Serialize)]
struct RoleSyncRequestData {
    pub account_id: i32,
    pub keep: Vec<String>,
    pub remove: Vec<String>,
}

pub enum RoleSyncError {
    NotLinked,
    Database(sqlx::Error),
    ServerRequest(reqwest::Error),
    #[allow(unused)]
    InternalError(&'static str),
    ServerUpdate((StatusCode, String)),
}

impl From<sqlx::Error> for RoleSyncError {
    fn from(value: sqlx::Error) -> Self {
        match value {
            sqlx::Error::RowNotFound => Self::NotLinked,
            v => Self::Database(v),
        }
    }
}

impl Display for RoleSyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotLinked => f.write_str("User not linked"),
            Self::Database(e) => write!(f, "Database error: {e}"),
            Self::ServerRequest(e) => write!(f, "Error making a request to the server: {e}"),
            Self::InternalError(e) => write!(f, "Internal error: {e}"),
            Self::ServerUpdate((code, message)) => {
                write!(f, "Server returned error (code {code}): {message}")
            }
        }
    }
}

impl BotState {
    pub fn new(database: sqlx::SqlitePool) -> Self {
        let mut base_url =
            env::var("BOT_BASE_URL").expect("'BOT_BASE_URL' env variable not passed");
        if base_url.ends_with('/') {
            base_url.pop();
        }

        let server_password =
            env::var("BOT_SERVER_PASSWORD").expect("'BOT_SERVER_PASSWORD' env variable not passed");

        let guild_id = GuildId::new(
            env::var("BOT_SERVER_ID")
                .expect("Expected BOT_SERVER_ID in environment")
                .parse()
                .expect("BOT_SERVER_ID must be an integer"),
        );

        Self {
            http_client: reqwest::Client::builder()
                .user_agent(format!(
                    "globed-game-server/discord-bot-{}",
                    env!("CARGO_PKG_VERSION")
                ))
                .build()
                .expect("Failed to create the HTTP client"),
            base_url,
            server_password,
            database,
            guild_id,
        }
    }

    pub async fn sync_roles(&self, user: &Member) -> Result<(), RoleSyncError> {
        let user_id = user.user.id.get() as i64;

        // check if the user is linked
        let linked_user = sqlx::query_as!(
            LinkedUser,
            "SELECT * FROM linked_users WHERE id = ?",
            user_id
        )
        .fetch_one(&self.database)
        .await?;

        // fetch roles from the database
        let db_roles = self.get_all_roles().await?;

        // depending on which roles the user has, make a vec of roles that should be kept, and roles that should be removed
        let mut kept = Vec::new();
        let mut removed = Vec::new();

        for role in db_roles {
            // check if user has that role on discord
            if user
                .roles
                .iter()
                .any(|id| id.get() as i64 == role.discord_id)
            {
                // add to list of roles to be kept
                kept.push(role.id);
            } else {
                // add to list of roles to be removed
                removed.push(role.id);
            }
        }

        #[cfg(debug_assertions)]
        debug!(
            "for {}, keep: {kept:?}, remove: {removed:?}",
            user.display_name()
        );

        /* make a request to update the roles on the central server */
        let data = RoleSyncRequestData {
            account_id: linked_user.gd_account_id as i32,
            keep: kept,
            remove: removed,
        };

        self._send_sync_roles_req(&data).await
    }

    pub async fn handle_unlink(&self, user: &Member) -> Result<(), RoleSyncError> {
        let user_id = user.user.id.get() as i64;

        // check if the user is linked
        let linked_user = sqlx::query_as!(
            LinkedUser,
            "SELECT * FROM linked_users WHERE id = ?",
            user_id
        )
        .fetch_one(&self.database)
        .await?;

        // fetch roles from the database
        let db_roles = sqlx::query_as!(Role, "SELECT * FROM roles")
            .fetch_all(&self.database)
            .await?;

        // remove user from the database
        sqlx::query!("DELETE FROM linked_users WHERE id = ?", user_id)
            .execute(&self.database)
            .await?;

        // sync roles with the server

        let mut removed = Vec::new();

        for role in db_roles {
            removed.push(role.id);
        }

        let data = RoleSyncRequestData {
            account_id: linked_user.gd_account_id as i32,
            keep: Vec::new(),
            remove: removed,
        };

        self._send_sync_roles_req(&data).await
    }

    // internal function for making server web request to sync roles
    async fn _send_sync_roles_req(&self, data: &RoleSyncRequestData) -> Result<(), RoleSyncError> {
        let body = match serde_json::to_string(data) {
            Ok(x) => x,
            Err(err) => {
                error!("This should never fail: {err}");

                #[cfg(debug_assertions)]
                unreachable!();
                #[cfg(not(debug_assertions))]
                return Err(RoleSyncError::InternalError(
                    "internal error in serializing data",
                ));
            }
        };

        let response = match self
            .http_client
            .post(format!("{}/gsp/sync_roles", self.base_url))
            .header("Authorization", &self.server_password)
            .body(body)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                return Err(RoleSyncError::ServerRequest(e));
            }
        };

        let status = response.status();
        if !status.is_success() {
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "<no message>".to_owned());

            warn!(
                "Role update failed: code {}, message: {}",
                status.as_u16(),
                message
            );

            return Err(RoleSyncError::ServerUpdate((status, message)));
        }

        // success!
        Ok(())
    }

    pub async fn add_role(&self, role_id: i64, globed_role_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query!(
            "INSERT INTO roles (id, discord_id) VALUES (?, ?)",
            globed_role_id,
            role_id
        )
        .execute(&self.database)
        .await
        .map(|_| ())
    }

    pub async fn remove_role(&self, role_id: i64) -> Result<(), sqlx::Error> {
        sqlx::query!("DELETE FROM roles WHERE discord_id = ?", role_id)
            .execute(&self.database)
            .await
            .map(|_| ())
    }

    pub async fn remove_role_by_globed_id(&self, role: &str) -> Result<(), sqlx::Error> {
        sqlx::query!("DELETE FROM roles WHERE id = ?", role)
            .execute(&self.database)
            .await
            .map(|_| ())
    }

    pub async fn get_all_roles(&self) -> Result<Vec<Role>, sqlx::Error> {
        sqlx::query_as!(Role, "SELECT * FROM roles")
            .fetch_all(&self.database)
            .await
    }
}
