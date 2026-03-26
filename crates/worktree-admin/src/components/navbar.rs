//! Navbar component using inline styles with shadcn CSS variables

use yew::prelude::*;
use yew_router::prelude::*;
use crate::{Route, utils::{colors, layout, border, shadow, typography, combine_styles}};

#[function_component(Navbar)]
pub fn navbar() -> Html {
    let navbar_style = combine_styles(&[
        colors::bg_card(),
        "border-bottom: 1px solid hsl(var(--border))",
        shadow::sm(),
        "position: sticky",
        "top: 0",
        "z-index: 50",
        "width: 100%",
    ]);

    let container_style = combine_styles(&[
        layout::flex(),
        "align-items: center",
        "justify-content: space-between",
        "max-width: 1400px",
        "margin: 0 auto",
        "padding: 1rem 2rem",
    ]);

    let brand_style = combine_styles(&[
        layout::flex(),
        "align-items: center",
        "gap: 0.75rem",
        colors::text_foreground(),
        "font-weight: 600",
        "font-size: 1.25rem",
        "text-decoration: none",
    ]);

    let logo_style = combine_styles(&[
        "font-size: 1.5rem",
        "line-height: 1",
    ]);

    let menu_style = combine_styles(&[
        layout::flex(),
        "align-items: center",
        "gap: 0.5rem",
        "list-style: none",
        "margin: 0",
        "padding: 0",
    ]);

    let link_style = combine_styles(&[
        layout::flex(),
        "align-items: center",
        "gap: 0.5rem",
        "padding: 0.5rem 1rem",
        border::rounded(),
        colors::text_foreground(),
        "text-decoration: none",
        "transition: all 150ms cubic-bezier(0.4, 0, 0.2, 1)",
        "font-weight: 500",
    ]);

    let link_hover_style = "background-color: hsl(var(--accent)); color: hsl(var(--accent-foreground))";

    let actions_style = combine_styles(&[
        layout::flex(),
        "align-items: center",
        "gap: 0.5rem",
    ]);

    let button_style = combine_styles(&[
        "display: flex",
        "align-items: center",
        "justify-content: center",
        "width: 2.5rem",
        "height: 2.5rem",
        border::rounded(),
        "border: none",
        colors::bg_secondary(),
        colors::text_foreground(),
        "cursor: pointer",
        "transition: all 150ms cubic-bezier(0.4, 0, 0.2, 1)",
        "font-size: 1.25rem",
    ]);

    html! {
        <nav style={navbar_style}>
            <div style={container_style}>
                <Link<Route> to={Route::Dashboard} classes="navbar-brand">
                    <div style={brand_style}>
                        <span style={logo_style}>{"🌳"}</span>
                        <span>{"Worktree Admin"}</span>
                    </div>
                </Link<Route>>

                <ul style={menu_style}>
                    <li>
                        <Link<Route> to={Route::Dashboard}>
                            <div style={link_style.clone()} class="nav-link">
                                <span>{"📊"}</span>
                                <span>{"Dashboard"}</span>
                            </div>
                        </Link<Route>>
                    </li>
                    <li>
                        <Link<Route> to={Route::Repositories}>
                            <div style={link_style.clone()} class="nav-link">
                                <span>{"📁"}</span>
                                <span>{"Repositories"}</span>
                            </div>
                        </Link<Route>>
                    </li>
                    <li>
                        <Link<Route> to={Route::Statistics}>
                            <div style={link_style.clone()} class="nav-link">
                                <span>{"📈"}</span>
                                <span>{"Statistics"}</span>
                            </div>
                        </Link<Route>>
                    </li>
                    <li>
                        <Link<Route> to={Route::Settings}>
                            <div style={link_style.clone()} class="nav-link">
                                <span>{"⚙️"}</span>
                                <span>{"Settings"}</span>
                            </div>
                        </Link<Route>>
                    </li>
                </ul>

                <div style={actions_style}>
                    <button
                        style={button_style.clone()}
                        title="Toggle theme"
                        class="theme-toggle"
                    >
                        {"🌙"}
                    </button>
                    <button
                        style={button_style}
                        title="Refresh"
                        class="refresh-button"
                    >
                        {"🔄"}
                    </button>
                </div>
            </div>
        </nav>
    }
}
