use anyhow::Result;
use clap::Subcommand;

use super::{output, Ctx};
use crate::library::papers;
use crate::library::projects::{self, ProjectName};

#[derive(Subcommand)]
pub(super) enum ProjectCmd {
    /// List projects with paper counts.
    List,
    /// Create a new project.
    New { name: String },
    /// Rename a project.
    Rename { project: String, name: String },
    /// Delete a project (papers are kept).
    Rm { project: String },
    /// Add one or more papers to a project.
    Add {
        project: String,
        #[arg(required = true)]
        papers: Vec<String>,
    },
    /// Remove a paper from a project.
    Remove { project: String, paper: String },
    /// List the papers in a project.
    Show { project: String },
}

pub(super) async fn project(ctx: &Ctx, cmd: ProjectCmd) -> Result<()> {
    let pool = &ctx.pool;
    match cmd {
        ProjectCmd::List => {
            let projects = projects::list(pool).await?;
            if projects.is_empty() {
                println!("no projects");
            }
            for s in projects {
                println!("{}  ({} papers)", s.project.name, s.paper_count);
            }
        }
        ProjectCmd::New { name } => {
            let name = project_name(&name)?;
            let p = projects::create(pool, &name).await.map_err(name_taken)?;
            println!("created project {} ({})", p.name, p.id);
        }
        ProjectCmd::Rename { project, name } => {
            let p = projects::find_one(pool, &project).await?;
            let name = project_name(&name)?;
            let renamed = projects::update(pool, &p.id, Some(&name))
                .await
                .map_err(name_taken)?
                .ok_or_else(|| anyhow::anyhow!("project {} was deleted meanwhile", p.name))?;
            println!("renamed project {} to {}", p.name, renamed.name);
        }
        ProjectCmd::Rm { project } => {
            let p = projects::find_one(pool, &project).await?;
            projects::delete(pool, &p.id).await?;
            println!("deleted project {}", p.name);
        }
        ProjectCmd::Add { project, papers } => {
            let proj = projects::find_one(pool, &project).await?;
            for sel in &papers {
                let paper = papers::find_one(pool, sel).await?;
                projects::add_paper(pool, &paper.id, &proj.id).await?;
                let label = paper.cite_key.as_deref().unwrap_or(&paper.id);
                println!("added {label} to {}", proj.name);
            }
        }
        ProjectCmd::Remove { project, paper } => {
            let proj = projects::find_one(pool, &project).await?;
            let paper = papers::find_one(pool, &paper).await?;
            let label = paper.cite_key.as_deref().unwrap_or(&paper.id);
            if projects::remove_paper(pool, &paper.id, &proj.id).await? {
                println!("removed {label} from {}", proj.name);
            } else {
                println!("{label} was not in {}", proj.name);
            }
        }
        ProjectCmd::Show { project } => {
            let proj = projects::find_one(pool, &project).await?;
            let filter = papers::PaperFilter {
                project: Some(proj.id.clone()),
                ..Default::default()
            };
            let papers = papers::list(pool, None, None, &filter).await?;
            println!("{} — {} paper(s)", proj.name, papers.len());
            for p in papers {
                println!("  {}  {}", p.id, output::title(p.meta.title.as_deref()));
            }
        }
    }
    Ok(())
}

fn project_name(raw: &str) -> Result<ProjectName> {
    ProjectName::parse(raw).map_err(|_| anyhow::anyhow!("project name cannot be empty"))
}

fn name_taken(e: anyhow::Error) -> anyhow::Error {
    if crate::db::is_unique_violation(&e) {
        anyhow::anyhow!(projects::NAME_TAKEN)
    } else {
        e
    }
}
