//! Loading spinner component for Worktree Admin Panel

use yew::prelude::*;

/// Props for the Loading component
#[derive(Properties, PartialEq)]
pub struct LoadingProps {
    /// Optional message to display below the spinner
    #[prop_or_default]
    pub message: Option<String>,

    /// Size variant: "small", "medium", "large"
    #[prop_or("medium".to_string())]
    pub size: String,

    /// Whether to show as fullscreen overlay
    #[prop_or(false)]
    pub fullscreen: bool,
}

/// Loading spinner component
///
/// # Examples
///
/// ```rust
/// use yew::prelude::*;
/// use worktree_admin::components::Loading;
///
/// #[function_component(MyComponent)]
/// fn my_component() -> Html {
///     html! {
///         <Loading message="Loading data..." />
///     }
/// }
/// ```
#[function_component(Loading)]
pub fn loading(props: &LoadingProps) -> Html {
    let container_class = if props.fullscreen {
        "loading-container loading-fullscreen"
    } else {
        "loading-container"
    };

    let spinner_class = format!("loading-spinner loading-{}", props.size);

    html! {
        <div class={container_class}>
            <div class="loading-content">
                <div class={spinner_class}>
                    <div class="spinner-circle"></div>
                    <div class="spinner-circle"></div>
                    <div class="spinner-circle"></div>
                </div>
                if let Some(message) = &props.message {
                    <p class="loading-message">{message}</p>
                }
            </div>
        </div>
    }
}

/// Simple loading spinner without props
#[function_component(SimpleLoading)]
pub fn simple_loading() -> Html {
    html! {
        <Loading />
    }
}

/// Fullscreen loading overlay
#[function_component(FullscreenLoading)]
pub fn fullscreen_loading() -> Html {
    html! {
        <Loading fullscreen={true} message="Loading..." />
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loading_props() {
        let props = LoadingProps {
            message: Some("Test".to_string()),
            size: "large".to_string(),
            fullscreen: true,
        };

        assert_eq!(props.message, Some("Test".to_string()));
        assert_eq!(props.size, "large");
        assert_eq!(props.fullscreen, true);
    }
}
