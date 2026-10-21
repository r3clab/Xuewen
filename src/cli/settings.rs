use anyhow::Result;

use super::Ctx;
use crate::app::error::Error;
use crate::app::settings::PROXY_COOKIE;
use crate::library::settings;

#[derive(clap::Args)]
pub(super) struct ProxyCookieArgs {
    /// Store this cookie value (a `name=value; name2=value2` header string).
    #[arg(long, conflicts_with = "clear")]
    set: Option<String>,
    /// Remove the stored cookie.
    #[arg(long)]
    clear: bool,
}

pub(super) async fn proxy_cookie(ctx: &Ctx, args: ProxyCookieArgs) -> Result<()> {
    let app = &ctx.app;
    if args.clear {
        app.clear_proxy_cookie().await?;
        println!("proxy cookie cleared");
    } else if let Some(cookie) = args.set {
        app.set_proxy_cookie(&cookie).await.map_err(|e| match e {
            Error::Invalid(_) => anyhow::anyhow!(
                "empty cookie — to remove the stored cookie, run: xuewen proxy-cookie --clear"
            ),
            e => e.into(),
        })?;
        println!("proxy cookie stored");
    } else {
        match settings::updated_at(&ctx.pool, PROXY_COOKIE).await? {
            Some(ts) => println!("proxy cookie set (updated {ts})"),
            None => println!("no proxy cookie set"),
        }
    }
    Ok(())
}
