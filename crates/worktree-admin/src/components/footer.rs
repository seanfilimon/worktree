//! Footer component for Worktree Admin Panel

use yew::prelude::*;

#[function_component(Footer)]
pub fn footer() -> Html {
    let current_year = chrono::Utc::now().year();

    html! {
        <footer class="footer">
            <div class="footer-container">
                <div class="footer-content">
                    <div class="footer-section">
                        <p class="footer-copyright">
                            {"© "}{current_year}{" Worktree Admin. All rights reserved."}
                        </p>
                    </div>

                    <div class="footer-section footer-links">
                        <a href="https://github.com/worktree" class="footer-link" target="_blank" rel="noopener noreferrer">
                            {"GitHub"}
                        </a>
                        <span class="footer-separator">{"•"}</span>
                        <a href="/docs" class="footer-link">
                            {"Documentation"}
                        </a>
                        <span class="footer-separator">{"•"}</span>
                        <a href="/api" class="footer-link">
                            {"API"}
                        </a>
                        <span class="footer-separator">{"•"}</span>
                        <a href="/support" class="footer-link">
                            {"Support"}
                        </a>
                    </div>

                    <div class="footer-section footer-info">
                        <span class="footer-version">{"v1.0.0"}</span>
                    </div>
                </div>
            </div>
        </footer>
    }
}
