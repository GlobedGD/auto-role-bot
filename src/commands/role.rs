use super::prelude::*;

#[poise::command(slash_command, subcommands("add", "remove", "removeid", "list"))]
pub async fn role(_ctx: Context<'_>) -> Result<(), CommandError> {
    // unreachable
    Ok(())
}

/// Add a new linked role
#[poise::command(slash_command)]
pub async fn add(
    ctx: Context<'_>,
    #[description = "Role to add"] role: serenity::Role,
    #[description = "Role ID on the Globed server"] globed_role_id: String,
) -> Result<(), CommandError> {
    let state = ctx.data();

    if !has_admin_perm(&ctx).await {
        ctx.reply(":x: No permission").await?;
        return Ok(());
    }

    match state.add_role(role.id.get() as i64, &globed_role_id).await {
        Ok(()) => {
            ctx.reply(format!(
                "✅ Successfully linked role <@&{}> to globed role `{}`.",
                role.id, globed_role_id
            ))
            .await?
        }
        Err(e) => {
            ctx.reply(format!(":x: Failed to add the role: {e}"))
                .await?
        }
    };

    Ok(())
}

/// Remove a linked role
#[poise::command(slash_command)]
pub async fn remove(
    ctx: Context<'_>,
    #[description = "Role to remove"] role: serenity::Role,
) -> Result<(), CommandError> {
    let state = ctx.data();

    if !has_admin_perm(&ctx).await {
        ctx.reply(":x: No permission").await?;
        return Ok(());
    }

    match state.remove_role(role.id.get() as i64).await {
        Ok(()) => {
            ctx.reply(format!("✅ Successfully removed role <@&{}>.", role.id))
                .await?;
        }
        Err(RoleRemoveError::Database(e)) => {
            ctx.reply(format!(":x: Failed to remove the role: {e}"))
                .await?;
            bail!("Role removal failed: {e}");
        }

        Err(RoleRemoveError::NotFound) => {
            ctx.reply(":x: Role is not currently linked to any role on Globed.")
                .await?;
        }
    };

    Ok(())
}

/// Remove a linked role by Globed ID
#[poise::command(slash_command)]
pub async fn removeid(
    ctx: Context<'_>,
    #[description = "Role to remove"] globed_role_id: String,
) -> Result<(), CommandError> {
    let state = ctx.data();

    if !has_admin_perm(&ctx).await {
        ctx.reply(":x: No permission").await?;
        return Ok(());
    }

    match state.remove_role_by_globed_id(&globed_role_id).await {
        Ok(()) => {
            ctx.reply(format!(
                "✅ Successfully removed role `{}`.",
                globed_role_id
            ))
            .await?;
        }

        Err(RoleRemoveError::Database(e)) => {
            ctx.reply(format!(":x: Failed to remove the role: {e}"))
                .await?;
            bail!("Role removal failed: {e}");
        }

        Err(RoleRemoveError::NotFound) => {
            ctx.reply(":x: Role is not currently linked to any role on Globed.")
                .await?;
        }
    };

    Ok(())
}

/// List all linked roles
#[poise::command(slash_command)]
pub async fn list(ctx: Context<'_>) -> Result<(), CommandError> {
    let state = ctx.data();

    if !has_manage_roles_perm(&ctx).await {
        ctx.reply(":x: No permission").await?;
        return Ok(());
    }

    match state.get_all_roles().await {
        Ok(roles) => {
            let mut msg = "List of linked roles on this server:\n\n".to_owned();
            for role in roles {
                msg += &format!("* <@&{}> - `{}`\n", role.discord_id, role.id);
            }

            ctx.reply(msg).await?;
        }
        Err(e) => {
            ctx.reply(format!(":x: Failed to get the list of roles: {e}"))
                .await?;
        }
    };

    Ok(())
}
