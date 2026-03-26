//! Repository card component for displaying repository information

use yew::prelude::*;
use crate::types::RepositoryInfo;
use crate::components::{Card, Badge, BadgeVariant};

#[derive(Properties, Clone, PartialEq)]
pub struct RepoCardProps {
    pub repo: RepositoryInfo,
    #[prop_or_default]
    pub on_click: Option<Callback<RepositoryInfo>>,
}

/// Repository card component displaying repository information with status badge
#[function_component(RepoCard)]
pub fn repo_card(props: &RepoCardProps) -> Html {
    let repo = props.repo.clone();
    let onclick = props.on_click.clone();

    let handle_click = {
        let repo = repo.clone();
        Callback::from(move |_| {
            if let Some(cb) = &onclick {
                cb.emit(repo.clone());
            }
        })
    };

    let format_size = |bytes: u64| -> String {
        const KB: u64 = 1024;
        const MB: u64 = KB * 1024;
        const GB: u64 = MB * 1024;

        if bytes >= GB {
            format!("{:.2} GB", bytes as f64 / GB as f64)
        } else if bytes >= MB {
            format!("{:.2} MB", bytes as f64 / MB as f64)
        } else if bytes >= KB {
            format!("{:.2} KB", bytes as f64 / KB as f64)
        } else {
            format!("{} B", bytes)
        }
    };

    let format_date = |date: chrono::DateTime<chrono::Utc>| -> String {
        let now = chrono::Utc::now();
        let duration = now.signed_duration_since(date);

        if duration.num_days() > 0 {
            format!("{} days ago", duration.num_days())
        } else if duration.num_hours() > 0 {
            format!("{} hours ago", duration.num_hours())
        } else if duration.num_minutes() > 0 {
            format!("{} minutes ago", duration.num_minutes())
        } else {
            "just now".to_string()
        }
    };

    let badge_variant = match repo.status {
        crate::types::RepositoryStatus::Active => BadgeVariant::Success,
        crate::types::RepositoryStatus::Idle => BadgeVariant::Secondary,
        crate::types::RepositoryStatus::Syncing => BadgeVariant::Warning,
        crate::types::RepositoryStatus::Error => BadgeVariant::Error,
    };

    html! {
        <Card title={repo.name.clone()}>
            <div class="repo-card" onclick={handle_click}>
                <div class="repo-card-header">
                    <h3 class="repo-card-title">{&repo.name}</h3>
                    <Badge variant={badge_variant}>{repo.status.label()}</Badge>
                </div>

                <div class="repo-card-path">
                    <span class="repo-card-icon">{"📁"}</span>
                    <span class="repo-card-path-text" title={repo.path.clone()}>
                        {&repo.path}
                    </span>
                </div>

                <div class="repo-card-stats">
                    <div class="repo-card-stat">
                        <span class="repo-card-stat-icon">{"🌿"}</span>
                        <span class="repo-card-stat-value">{repo.branch_count}</span>
                        <span class="repo-card-stat-label">
                            {if repo.branch_count == 1 { "branch" } else { "branches" }}
                        </span>
                    </div>

                    <div class="repo-card-stat">
                        <span class="repo-card-stat-icon">{"📝"}</span>
                        <span class="repo-card-stat-value">{repo.commit_count}</span>
                        <span class="repo-card-stat-label">
                            {if repo.commit_count == 1 { "commit" } else { "commits" }}
                        </span>
                    </div>

                    <div class="repo-card-stat">
                        <span class="repo-card-stat-icon">{"💾"}</span>
                        <span class="repo-card-stat-value">{format_size(repo.size_bytes)}</span>
                        <span class="repo-card-stat-label">{"size"}</span>
                    </div>
                </div>

                <div class="repo-card-footer">
                    <span class="repo-card-activity">
                        <span class="repo-card-activity-icon">{"🕒"}</span>
                        <span class="repo-card-activity-text">
                            {"Last activity: "}{format_date(repo.last_activity)}
                        </span>
                    </span>
                </div>
            </div>
        </Card>
    }
}
