use std::io::Read as _;

use anyhow::{Context as _, Result};

use super::Ctx;
use crate::config::TranslateProvider;

#[derive(clap::Args)]
pub(super) struct TranslateArgs {
    /// Text to translate; `-` or nothing reads it from stdin.
    text: Vec<String>,
    /// Target language (default: [translate].target_lang).
    #[arg(long)]
    to: Option<String>,
    /// llm or deepl (default: [translate].provider, else whichever is
    /// configured).
    #[arg(long)]
    provider: Option<TranslateProvider>,
}

pub(super) async fn translate(ctx: &Ctx, args: TranslateArgs) -> Result<()> {
    let svc = ctx.app.translate()?;
    let text = match args.text.as_slice() {
        [] => read_stdin()?,
        [dash] if dash == "-" => read_stdin()?,
        words => words.join(" "),
    };
    let translated = svc
        .translate(&text, args.to.as_deref(), args.provider)
        .await?;
    println!("{}", translated.text);
    Ok(())
}

fn read_stdin() -> Result<String> {
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .context("reading the text from stdin")?;
    Ok(text)
}
