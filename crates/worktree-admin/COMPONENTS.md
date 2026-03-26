# Worktree Admin Components

This document provides an overview and usage examples for all reusable UI components in the Worktree Admin Panel.

## Table of Contents

1. [Footer](#footer)
2. [Card](#card)
3. [Badge](#badge)
4. [Button](#button)
5. [StatCard](#statcard)
6. [RepoCard](#repocard)
7. [Loading](#loading)

---

## Footer

A simple footer component with copyright information and navigation links.

### Usage

```rust
use yew::prelude::*;
use worktree_admin::components::Footer;

#[function_component(App)]
fn app() -> Html {
    html! {
        <div>
            <main>
                // Your content here
            </main>
            <Footer />
        </div>
    }
}
```

### Features
- Automatic current year detection
- Quick links (GitHub, Documentation, API, Support)
- Version information display

---

## Card

A generic card container component with optional title and header actions.

### Props

| Prop | Type | Default | Description |
|------|------|---------|-------------|
| `title` | `Option<AttrValue>` | `None` | Optional card title |
| `class` | `Classes` | `Classes::default()` | Additional CSS classes |
| `children` | `Children` | Required | Card content |
| `header_actions` | `Option<Html>` | `None` | Optional buttons/actions in header |

### Usage

```rust
use yew::prelude::*;
use worktree_admin::components::Card;

#[function_component(Example)]
fn example() -> Html {
    html! {
        <Card title="User Information">
            <p>{"Name: John Doe"}</p>
            <p>{"Email: john@example.com"}</p>
        </Card>
    }
}
```

### With Header Actions

```rust
html! {
    <Card 
        title="Settings"
        header_actions={html! {
            <button>{"Edit"}</button>
        }}
    >
        <p>{"Configuration options..."}</p>
    </Card>
}
```

### CardFooter

Optional footer component for cards:

```rust
use worktree_admin::components::{Card, CardFooter};

html! {
    <Card title="Article">
        <p>{"Article content..."}</p>
        <CardFooter>
            <button>{"Like"}</button>
            <button>{"Share"}</button>
        </CardFooter>
    </Card>
}
```

---

## Badge

A badge component for displaying status indicators and labels with various color variants.

### Variants

- `BadgeVariant::Success` - Green (for success states)
- `BadgeVariant::Warning` - Yellow/Orange (for warnings)
- `BadgeVariant::Error` - Red (for errors)
- `BadgeVariant::Secondary` - Gray (neutral/inactive)
- `BadgeVariant::Primary` - Blue (primary actions)
- `BadgeVariant::Info` - Light blue (informational)

### Props

| Prop | Type | Default | Description |
|------|------|---------|-------------|
| `children` | `Children` | Required | Badge text content |
| `variant` | `BadgeVariant` | `Secondary` | Color variant |
| `class` | `Classes` | `Classes::default()` | Additional CSS classes |
| `outlined` | `bool` | `false` | Show outline style |
| `pill` | `bool` | `false` | Show rounded pill style |

### Usage

```rust
use yew::prelude::*;
use worktree_admin::components::{Badge, BadgeVariant};

#[function_component(Example)]
fn example() -> Html {
    html! {
        <div>
            <Badge variant={BadgeVariant::Success}>{"Active"}</Badge>
            <Badge variant={BadgeVariant::Warning}>{"Pending"}</Badge>
            <Badge variant={BadgeVariant::Error}>{"Failed"}</Badge>
            <Badge variant={BadgeVariant::Secondary}>{"Inactive"}</Badge>
        </div>
    }
}
```

### Styled Variants

```rust
html! {
    <>
        <Badge variant={BadgeVariant::Primary} outlined={true}>
            {"Outlined"}
        </Badge>
        <Badge variant={BadgeVariant::Success} pill={true}>
            {"Pill Style"}
        </Badge>
    </>
}
```

---

## Button

A button component with multiple variants and full event handling support.

### Variants

- `ButtonVariant::Primary` - Primary action button
- `ButtonVariant::Secondary` - Secondary action button
- `ButtonVariant::Danger` - Destructive action button

### Props

| Prop | Type | Default | Description |
|------|------|---------|-------------|
| `children` | `Children` | Required | Button content |
| `variant` | `ButtonVariant` | `Primary` | Button style variant |
| `onclick` | `Callback<MouseEvent>` | No-op | Click event handler |
| `disabled` | `bool` | `false` | Whether button is disabled |
| `button_type` | `String` | `"button"` | HTML button type attribute |
| `class` | `Classes` | `Classes::default()` | Additional CSS classes |

### Usage

```rust
use yew::prelude::*;
use worktree_admin::components::{Button, ButtonVariant};

#[function_component(Example)]
fn example() -> Html {
    let on_save = Callback::from(|_| {
        log::info!("Save clicked!");
    });

    let on_delete = Callback::from(|_| {
        log::warn!("Delete clicked!");
    });

    html! {
        <div>
            <Button variant={ButtonVariant::Primary} onclick={on_save}>
                {"Save"}
            </Button>
            <Button variant={ButtonVariant::Secondary}>
                {"Cancel"}
            </Button>
            <Button variant={ButtonVariant::Danger} onclick={on_delete}>
                {"Delete"}
            </Button>
        </div>
    }
}
```

### Disabled State

```rust
html! {
    <Button disabled={true}>
        {"Loading..."}
    </Button>
}
```

---

## StatCard

A statistics card component for displaying key metrics with icons.

### Props

| Prop | Type | Default | Description |
|------|------|---------|-------------|
| `icon` | `AttrValue` | Required | Icon (emoji or text) |
| `value` | `AttrValue` | Required | The statistic value |
| `label` | `AttrValue` | Required | Label describing the stat |
| `trend` | `Option<AttrValue>` | `None` | Optional trend indicator |
| `class` | `Classes` | `Classes::default()` | Additional CSS classes |

### Usage

```rust
use yew::prelude::*;
use worktree_admin::components::StatCard;

#[function_component(Dashboard)]
fn dashboard() -> Html {
    html! {
        <div class="stats-grid">
            <StatCard
                icon="📁"
                value="127"
                label="Total Repositories"
                trend="+12%"
            />
            <StatCard
                icon="💾"
                value="2.4 GB"
                label="Total Storage"
                trend="+8%"
            />
            <StatCard
                icon="🌿"
                value="384"
                label="Active Branches"
            />
            <StatCard
                icon="📝"
                value="15,234"
                label="Total Commits"
                trend="+142"
            />
        </div>
    }
}
```

---

## RepoCard

A specialized card component for displaying repository information with status badges.

### Props

| Prop | Type | Default | Description |
|------|------|---------|-------------|
| `repo` | `RepositoryInfo` | Required | Repository data |
| `on_click` | `Option<Callback<RepositoryInfo>>` | `None` | Click handler |

### Usage

```rust
use yew::prelude::*;
use worktree_admin::components::RepoCard;
use worktree_admin::types::RepositoryInfo;

#[function_component(RepositoryList)]
fn repository_list() -> Html {
    let repositories = vec![
        RepositoryInfo::mock("worktree-server"),
        RepositoryInfo::mock("worktree-admin"),
    ];

    let handle_repo_click = Callback::from(|repo: RepositoryInfo| {
        log::info!("Clicked repository: {}", repo.name);
    });

    html! {
        <div class="repo-grid">
            { for repositories.iter().map(|repo| {
                html! {
                    <RepoCard
                        repo={repo.clone()}
                        on_click={handle_repo_click.clone()}
                    />
                }
            })}
        </div>
    }
}
```

### Features

- Displays repository name, path, and status badge
- Shows branch count, commit count, and size
- Displays last activity timestamp with relative time
- Automatic size formatting (B, KB, MB, GB)
- Automatic date formatting (minutes, hours, days ago)
- Click handling support

---

## Loading

A loading spinner component with multiple size variants and fullscreen support.

### Props

| Prop | Type | Default | Description |
|------|------|---------|-------------|
| `message` | `Option<String>` | `None` | Optional loading message |
| `size` | `String` | `"medium"` | Size: "small", "medium", "large" |
| `fullscreen` | `bool` | `false` | Show as fullscreen overlay |

### Usage

```rust
use yew::prelude::*;
use worktree_admin::components::Loading;

#[function_component(DataFetcher)]
fn data_fetcher() -> Html {
    let loading = use_state(|| true);

    if *loading {
        return html! {
            <Loading message="Loading repositories..." />
        };
    }

    html! {
        <div>{"Data loaded!"}</div>
    }
}
```

### Size Variants

```rust
html! {
    <>
        <Loading size="small" />
        <Loading size="medium" />
        <Loading size="large" />
    </>
}
```

### Fullscreen Loading

```rust
use worktree_admin::components::FullscreenLoading;

html! {
    <FullscreenLoading />
}
```

### Simple Loading (No Props)

```rust
use worktree_admin::components::SimpleLoading;

html! {
    <SimpleLoading />
}
```

---

## Complete Example

Here's a complete example using multiple components together:

```rust
use yew::prelude::*;
use worktree_admin::components::*;
use worktree_admin::types::RepositoryInfo;

#[function_component(Dashboard)]
fn dashboard() -> Html {
    let loading = use_state(|| false);
    let repositories = vec![
        RepositoryInfo::mock("project-alpha"),
        RepositoryInfo::mock("project-beta"),
    ];

    let handle_refresh = {
        let loading = loading.clone();
        Callback::from(move |_| {
            loading.set(true);
            // Trigger data refresh...
        })
    };

    if *loading {
        return html! { <Loading message="Refreshing data..." fullscreen={true} /> };
    }

    html! {
        <div class="dashboard">
            <Card title="Statistics">
                <div class="stats-grid">
                    <StatCard icon="📁" value="24" label="Repositories" trend="+3" />
                    <StatCard icon="🌿" value="156" label="Branches" />
                    <StatCard icon="📝" value="8,429" label="Commits" trend="+127" />
                    <StatCard icon="💾" value="4.2 GB" label="Storage" />
                </div>
            </Card>

            <Card 
                title="Recent Repositories"
                header_actions={html! {
                    <Button variant={ButtonVariant::Primary} onclick={handle_refresh}>
                        {"Refresh"}
                    </Button>
                }}
            >
                <div class="repo-list">
                    { for repositories.iter().map(|repo| {
                        html! { <RepoCard repo={repo.clone()} /> }
                    })}
                </div>
            </Card>

            <Footer />
        </div>
    }
}
```

---

## Styling

All components use CSS classes that can be styled according to your design system. The main class names are:

- `.footer`, `.footer-container`, `.footer-link`
- `.card`, `.card-header`, `.card-title`, `.card-body`, `.card-footer`
- `.badge`, `.badge-success`, `.badge-warning`, `.badge-error`, `.badge-secondary`
- `.btn`, `.btn-primary`, `.btn-secondary`, `.btn-danger`
- `.stat-card`, `.stat-card-icon`, `.stat-card-value`, `.stat-card-label`
- `.repo-card`, `.repo-card-header`, `.repo-card-stats`, `.repo-card-stat`
- `.loading-container`, `.loading-spinner`, `.loading-message`

Make sure to define appropriate styles in your CSS files to match your application's design.

---

## Best Practices

1. **Card Usage**: Use `Card` components to group related content and maintain visual consistency
2. **Badge Colors**: Choose badge variants that match the semantic meaning (success=green, error=red, etc.)
3. **Button Variants**: Use `Primary` for main actions, `Secondary` for alternatives, and `Danger` for destructive actions
4. **Loading States**: Always show loading indicators during async operations to improve UX
5. **StatCards**: Use consistent icons and formatting across all stat cards for visual harmony
6. **RepoCards**: Leverage the click handler for navigation to repository detail pages

---

## Contributing

When adding new components:
1. Create the component file in `src/components/`
2. Export it from `src/components/mod.rs`
3. Add comprehensive documentation with examples
4. Include prop documentation and usage examples
5. Add CSS class documentation
6. Write unit tests for complex logic

---

## License

MIT License - See LICENSE file for details