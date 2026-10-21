use anyhow::Result;
use clap::Subcommand;

use super::output::{confirm, ellipsize};
use super::Ctx;
use crate::annotations::{AnnotationColor, AnnotationKind, NewAnnotation};
use crate::app::error::Error;
use crate::library::papers;

#[derive(Subcommand)]
pub(super) enum AnnotationCmd {
    /// List a paper's annotations in reading order.
    List { paper: String },
    /// Attach a note to a page.
    ///
    /// The note is searchable (`in:notes`) and listed in the reader's
    /// annotation panel, but not drawn on the page: a CLI note has no position
    /// on it.
    Add {
        paper: String,
        note: String,
        /// Page number, 1-based.
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        page: u32,
        /// The passage the note is about.
        #[arg(long)]
        quote: Option<String>,
        /// The note's color in the reader's panel.
        #[arg(long, value_enum, default_value_t = ColorArg::Amber)]
        color: ColorArg,
        /// How the reader lists the note.
        #[arg(long, value_enum, default_value_t = KindArg::TextComment)]
        kind: KindArg,
    },
    /// Delete one annotation by id.
    Rm { paper: String, id: String },
    /// Delete every annotation on a paper.
    Clear {
        paper: String,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub(super) enum ColorArg {
    Amber,
    Rose,
    Green,
    Blue,
    Violet,
}

impl From<ColorArg> for AnnotationColor {
    fn from(c: ColorArg) -> Self {
        match c {
            ColorArg::Amber => Self::Amber,
            ColorArg::Rose => Self::Rose,
            ColorArg::Green => Self::Green,
            ColorArg::Blue => Self::Blue,
            ColorArg::Violet => Self::Violet,
        }
    }
}

#[derive(Clone, Copy, clap::ValueEnum)]
#[value(rename_all = "snake_case")]
pub(super) enum KindArg {
    Highlight,
    Underline,
    Strikeout,
    Squiggly,
    TextComment,
}

impl From<KindArg> for AnnotationKind {
    fn from(k: KindArg) -> Self {
        match k {
            KindArg::Highlight => Self::Highlight,
            KindArg::Underline => Self::Underline,
            KindArg::Strikeout => Self::Strikeout,
            KindArg::Squiggly => Self::Squiggly,
            KindArg::TextComment => Self::TextComment,
        }
    }
}

pub(super) async fn annotation(ctx: &Ctx, cmd: AnnotationCmd) -> Result<()> {
    let pool = &ctx.pool;
    let app = &ctx.app;
    let svc = app.annotations();
    match cmd {
        AnnotationCmd::List { paper } => {
            let paper = papers::find_one(pool, &paper).await?;
            let items = svc.list(&paper.id).await?;
            if items.is_empty() {
                println!("no annotations");
            }
            for a in &items {
                // Pages are 0-based in storage and 1-based to a reader.
                println!(
                    "{}  p{}  {}/{}  {}",
                    a.id,
                    a.page_index + 1,
                    a.kind,
                    a.color,
                    ellipsize(a.quoted_text.as_deref().unwrap_or(""), 60)
                );
                if let Some(note) = &a.note {
                    println!("      note: {note}");
                }
            }
        }
        AnnotationCmd::Add {
            paper,
            note,
            page,
            quote,
            color,
            kind,
        } => {
            let paper = papers::find_one(pool, &paper).await?;
            if note.trim().is_empty() {
                anyhow::bail!("the note must not be empty");
            }
            // The reader mints its own ids; this one only has to be unique
            // within the paper. An empty payload is what keeps the reader
            // from trying to redraw the mark.
            let id = uuid::Uuid::now_v7().to_string();
            let new = NewAnnotation {
                page_index: i64::from(page) - 1,
                kind: kind.into(),
                color: color.into(),
                quoted_text: quote,
                note: Some(note),
                payload: serde_json::json!({}),
            };
            app.put_annotation(&paper.id, &id, new).await?;
            println!("added {id} on p{page}");
        }
        AnnotationCmd::Rm { paper, id } => {
            let paper = papers::find_one(pool, &paper).await?;
            match app.delete_annotation(&paper.id, &id).await {
                Ok(()) => println!("removed {id}"),
                Err(Error::NotFound(_)) => anyhow::bail!("no annotation {id} on {}", paper.id),
                Err(e) => return Err(e.into()),
            }
        }
        AnnotationCmd::Clear { paper, yes } => {
            let paper = papers::find_one(pool, &paper).await?;
            let n = svc.count(&paper.id).await?;
            if n == 0 {
                println!("no annotations");
            } else if yes || confirm(&format!("Delete {n} annotation(s)?"))? {
                println!("removed {}", app.clear_annotations(&paper.id).await?);
            } else {
                println!("cancelled");
            }
        }
    }
    Ok(())
}
