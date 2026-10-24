//! UI templates and components using Maud for server-side rendering

use maud::{html, Markup, DOCTYPE};
use crate::{format_bytes, format_duration, RepositoryInfo, RepositoryStatus, ServerStats, ServerStatus};
use chrono::{DateTime, Utc};

/// Base HTML layout
pub fn layout(title: &str, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { (title) " - Worktree Admin" }
                link rel="stylesheet" href="/static/styles.css";
                script src="/static/app.js" defer {}
            }
            body {
                (navbar())
                main class="container" {
                    (content)
                }
                (footer())
            }
        }
    }
}

/// Navigation bar component
fn navbar() -> Markup {
    html! {
        nav class="navbar" {
            div class="navbar-container" {
                div class="navbar-brand" {
                    a href="/" class="navbar-logo" {
                        span class="logo-icon" { "🌳" }
                        span class="logo-text" { "Worktree Admin" }
                    }
                }
                ul class="navbar-menu" {
                    li { a href="/" class="navbar-link" { "Dashboard" } }
                    li { a href="/repositories" class="navbar-link" { "Repositories" } }
                    li { a href="/stats" class="navbar-link" { "Statistics" } }
                    li { a href="/settings" class="navbar-link" { "Settings" } }
                }
            }
        }
    }
}

/// Footer component
fn footer() -> Markup {
    html! {
        footer class="footer" {
            div class="footer-container" {
                p { "Worktree Admin Panel v0.1.0" }
                p {
                    "Built with "
                    a href="https://www.rust-lang.org/" target="_blank" { "Rust" }
                    " and "
                    a href="https://maud.lambda.xyz/" target="_blank" { "Maud" }
                }
            }
        }
    }
}

/// Dashboard page
pub fn dashboard_page(status: &ServerStatus, stats: &ServerStats, recent_repos: &[RepositoryInfo]) -> Markup {
    layout("Dashboard", html! {
        div class="dashboard" {
            h1 class="page-title" { "Dashboard" }

            (status_cards(status, stats))

            div class="dashboard-grid" {
                div class="dashboard-section" {
                    (server_status_card(status))
                }
                div class="dashboard-section" {
                    (recent_repositories_card(recent_repos))
                }
            }

            div class="dashboard-section full-width" {
                (activity_chart())
            }
        }
    })
}

/// Status cards component
fn status_cards(status: &ServerStatus, stats: &ServerStats) -> Markup {
    html! {
        div class="status-cards" {
            div class="status-card" {
                div class="status-card-icon" { "🟢" }
                div class="status-card-content" {
                    div class="status-card-label" { "Server Status" }
                    div class="status-card-value" {
                        @if status.running {
                            span class="badge badge-success" { "Running" }
                        } @else {
                            span class="badge badge-error" { "Stopped" }
                        }
                    }
                }
            }

            div class="status-card" {
                div class="status-card-icon" { "📦" }
                div class="status-card-content" {
                    div class="status-card-label" { "Repositories" }
                    div class="status-card-value" { (stats.total_repositories) }
                }
            }

            div class="status-card" {
                div class="status-card-icon" { "💾" }
                div class="status-card-content" {
                    div class="status-card-label" { "Total Storage" }
                    div class="status-card-value" { (format_bytes(stats.total_storage_bytes)) }
                }
            }

            div class="status-card" {
                div class="status-card-icon" { "🔀" }
                div class="status-card-content" {
                    div class="status-card-label" { "Total Commits" }
                    div class="status-card-value" { (stats.total_commits) }
                }
            }
        }
    }
}

/// Server status card
fn server_status_card(status: &ServerStatus) -> Markup {
    html! {
        div class="card" {
            div class="card-header" {
                h2 class="card-title" { "Server Status" }
            }
            div class="card-body" {
                div class="status-row" {
                    span class="status-label" { "Server ID:" }
                    span class="status-value font-mono" { (status.id) }
                }
                div class="status-row" {
                    span class="status-label" { "Uptime:" }
                    span class="status-value" { (format_duration(status.uptime_seconds)) }
                }
                div class="status-row" {
                    span class="status-label" { "Active Connections:" }
                    span class="status-value" { (status.active_connections) }
                }
                div class="status-row" {
                    span class="status-label" { "Tracked Repositories:" }
                    span class="status-value" { (status.tracked_repositories) }
                }
                div class="status-row" {
                    span class="status-label" { "Last Updated:" }
                    span class="status-value" { (format_datetime(&status.last_updated)) }
                }
            }
            div class="card-footer" {
                button class="btn btn-primary" { "Restart Server" }
                button class="btn btn-secondary" { "View Logs" }
            }
        }
    }
}

/// Recent repositories card
fn recent_repositories_card(repos: &[RepositoryInfo]) -> Markup {
    html! {
        div class="card" {
            div class="card-header" {
                h2 class="card-title" { "Recent Activity" }
                a href="/repositories" class="card-link" { "View All →" }
            }
            div class="card-body" {
                @if repos.is_empty() {
                    div class="empty-state" {
                        p { "No repositories found" }
                    }
                } @else {
                    div class="repo-list" {
                        @for repo in repos.iter().take(5) {
                            (repository_list_item(repo))
                        }
                    }
                }
            }
        }
    }
}

/// Activity chart placeholder
fn activity_chart() -> Markup {
    html! {
        div class="card" {
            div class="card-header" {
                h2 class="card-title" { "Activity Overview" }
            }
            div class="card-body" {
                div class="chart-placeholder" {
                    p { "📊 Chart visualization (coming soon)" }
                    p class="text-muted" { "Commits, operations, and activity over time" }
                }
            }
        }
    }
}

/// Repositories page
pub fn repositories_page(repos: &[RepositoryInfo]) -> Markup {
    layout("Repositories", html! {
        div class="repositories-page" {
            div class="page-header" {
                h1 class="page-title" { "Repositories" }
                button class="btn btn-primary" { "+ Add Repository" }
            }

            div class="repositories-filter" {
                input type="search" placeholder="Search repositories..." class="search-input";
                select class="filter-select" {
                    option { "All Status" }
                    option { "Active" }
                    option { "Idle" }
                    option { "Syncing" }
                    option { "Error" }
                }
            }

            div class="repositories-grid" {
                @for repo in repos {
                    (repository_card(repo))
                }
            }
        }
    })
}

/// Repository card component
fn repository_card(repo: &RepositoryInfo) -> Markup {
    html! {
        div class="card repo-card" {
            div class="card-header" {
                h3 class="repo-name" {
                    span class="repo-icon" { "📁" }
                    (repo.name)
                }
                (repository_status_badge(&repo.status))
            }
            div class="card-body" {
                div class="repo-info" {
                    div class="repo-info-item" {
                        span class="label" { "Path:" }
                        span class="value font-mono text-small" { (repo.path) }
                    }
                    div class="repo-info-item" {
                        span class="label" { "Branches:" }
                        span class="value" { (repo.branch_count) }
                    }
                    div class="repo-info-item" {
                        span class="label" { "Commits:" }
                        span class="value" { (repo.commit_count) }
                    }
                    div class="repo-info-item" {
                        span class="label" { "Size:" }
                        span class="value" { (format_bytes(repo.size_bytes)) }
                    }
                    div class="repo-info-item" {
                        span class="label" { "Last Activity:" }
                        span class="value" { (format_datetime(&repo.last_activity)) }
                    }
                }
            }
            div class="card-footer" {
                a href={ "/repositories/" (repo.id) } class="btn btn-sm btn-secondary" { "View Details" }
                button class="btn btn-sm btn-secondary" { "Settings" }
            }
        }
    }
}

/// Repository list item (compact)
fn repository_list_item(repo: &RepositoryInfo) -> Markup {
    html! {
        div class="repo-list-item" {
            div class="repo-list-item-content" {
                span class="repo-icon" { "📁" }
                div class="repo-list-item-info" {
                    div class="repo-list-item-name" { (repo.name) }
                    div class="repo-list-item-meta text-muted" {
                        (repo.commit_count) " commits · "
                        (format_bytes(repo.size_bytes))
                    }
                }
            }
            (repository_status_badge(&repo.status))
        }
    }
}

/// Repository status badge
fn repository_status_badge(status: &RepositoryStatus) -> Markup {
    let badge_class = match status {
        RepositoryStatus::Active => "badge-success",
        RepositoryStatus::Idle => "badge-secondary",
        RepositoryStatus::Syncing => "badge-warning",
        RepositoryStatus::Error => "badge-error",
    };

    html! {
        span class={ "badge " (badge_class) } {
            (status.label())
        }
    }
}

/// Statistics page
pub fn stats_page(stats: &ServerStats) -> Markup {
    layout("Statistics", html! {
        div class="stats-page" {
            h1 class="page-title" { "Statistics" }

            div class="stats-overview" {
                div class="stats-card" {
                    div class="stats-card-icon" { "📦" }
                    div class="stats-card-content" {
                        div class="stats-card-value" { (stats.total_repositories) }
                        div class="stats-card-label" { "Total Repositories" }
                    }
                }

                div class="stats-card" {
                    div class="stats-card-icon" { "🔀" }
                    div class="stats-card-content" {
                        div class="stats-card-value" { (stats.total_commits) }
                        div class="stats-card-label" { "Total Commits" }
                    }
                }

                div class="stats-card" {
                    div class="stats-card-icon" { "🌿" }
                    div class="stats-card-content" {
                        div class="stats-card-value" { (stats.total_branches) }
                        div class="stats-card-label" { "Total Branches" }
                    }
                }

                div class="stats-card" {
                    div class="stats-card-icon" { "💾" }
                    div class="stats-card-content" {
                        div class="stats-card-value" { (format_bytes(stats.total_storage_bytes)) }
                        div class="stats-card-label" { "Total Storage" }
                    }
                }

                div class="stats-card" {
                    div class="stats-card-icon" { "⚡" }
                    div class="stats-card-content" {
                        div class="stats-card-value" { (stats.total_operations) }
                        div class="stats-card-label" { "Total Operations" }
                    }
                }
            }

            div class="stats-details" {
                div class="card" {
                    div class="card-header" {
                        h2 class="card-title" { "Detailed Breakdown" }
                    }
                    div class="card-body" {
                        div class="chart-placeholder" {
                            p { "📊 Detailed charts and graphs (coming soon)" }
                        }
                    }
                }
            }

            div class="stats-footer" {
                p class="text-muted" {
                    "Statistics collected at " (format_datetime(&stats.collected_at))
                }
            }
        }
    })
}

/// Settings page
pub fn settings_page() -> Markup {
    layout("Settings", html! {
        div class="settings-page" {
            h1 class="page-title" { "Settings" }

            div class="settings-section" {
                div class="card" {
                    div class="card-header" {
                        h2 class="card-title" { "General Settings" }
                    }
                    div class="card-body" {
                        form class="settings-form" {
                            div class="form-group" {
                                label for="title" { "Application Title" }
                                input type="text" id="title" name="title" value="Worktree Admin" class="form-input";
                            }

                            div class="form-group" {
                                label for="theme" { "Theme" }
                                select id="theme" name="theme" class="form-select" {
                                    option value="auto" selected { "Auto" }
                                    option value="light" { "Light" }
                                    option value="dark" { "Dark" }
                                }
                            }

                            div class="form-group" {
                                label for="refresh" { "Auto-refresh Interval (seconds)" }
                                input type="number" id="refresh" name="refresh" value="30" class="form-input";
                            }

                            div class="form-actions" {
                                button type="submit" class="btn btn-primary" { "Save Changes" }
                                button type="reset" class="btn btn-secondary" { "Reset" }
                            }
                        }
                    }
                }
            }

            div class="settings-section" {
                div class="card" {
                    div class="card-header" {
                        h2 class="card-title" { "Security" }
                    }
                    div class="card-body" {
                        form class="settings-form" {
                            div class="form-group" {
                                label {
                                    input type="checkbox" name="auth_enabled";
                                    " Enable Authentication"
                                }
                            }

                            div class="form-group" {
                                label for="api_key" { "API Key" }
                                input type="password" id="api_key" name="api_key" class="form-input";
                            }

                            div class="form-actions" {
                                button type="submit" class="btn btn-primary" { "Update Security" }
                            }
                        }
                    }
                }
            }
        }
    })
}

/// Error page
pub fn error_page(status_code: u16, message: &str) -> Markup {
    layout(&format!("Error {}", status_code), html! {
        div class="error-page" {
            div class="error-container" {
                h1 class="error-code" { (status_code) }
                p class="error-message" { (message) }
                a href="/" class="btn btn-primary" { "← Back to Dashboard" }
            }
        }
    })
}

/// Format DateTime as human-readable string
fn format_datetime(dt: &DateTime<Utc>) -> String {
    dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}
