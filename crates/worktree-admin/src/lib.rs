//! Worktree Admin Panel - Yew WebAssembly Frontend
//!
//! A modern web-based admin interface for managing Worktree instances,
//! built with Yew and WebAssembly for maximum performance.

pub mod components;
pub mod pages;
pub mod services;
pub mod types;
pub mod utils;

use yew::prelude::*;
use yew_router::prelude::*;

pub use types::*;

/// Main application routes
#[derive(Clone, Routable, PartialEq)]
pub enum Route {
    #[at("/")]
    Dashboard,
    #[at("/repositories")]
    Repositories,
    #[at("/repositories/:id")]
    RepositoryDetail { id: String },
    #[at("/stats")]
    Statistics,
    #[at("/settings")]
    Settings,
    #[not_found]
    #[at("/404")]
    NotFound,
}

/// Main application component
#[function_component(App)]
pub fn app() -> Html {
    html! {
        <BrowserRouter>
            <div class="app">
                <components::Navbar />
                <main class="main-content">
                    <Switch<Route> render={switch} />
                </main>
                <components::Footer />
            </div>
        </BrowserRouter>
    }
}

/// Route switcher
fn switch(routes: Route) -> Html {
    match routes {
        Route::Dashboard => html! { <pages::Dashboard /> },
        Route::Repositories => html! { <pages::Repositories /> },
        Route::RepositoryDetail { id } => html! { <pages::RepositoryDetail {id} /> },
        Route::Statistics => html! { <pages::Statistics /> },
        Route::Settings => html! { <pages::Settings /> },
        Route::NotFound => html! { <pages::NotFound /> },
    }
}

/// Initialize the application
pub fn run_app() {
    wasm_logger::init(wasm_logger::Config::default());
    log::info!("Starting Worktree Admin Panel");
    yew::Renderer::<App>::new().render();
}
