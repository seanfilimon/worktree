//! Badge component with color variants

use yew::prelude::*;

/// Badge color variant
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadgeVariant {
    Success,
    Warning,
    Error,
    Secondary,
    Primary,
    Info,
}

impl BadgeVariant {
    pub fn as_class(&self) -> &'static str {
        match self {
            Self::Success => "badge-success",
            Self::Warning => "badge-warning",
            Self::Error => "badge-error",
            Self::Secondary => "badge-secondary",
            Self::Primary => "badge-primary",
            Self::Info => "badge-info",
        }
    }
}

/// Badge component properties
#[derive(Properties, PartialEq)]
pub struct BadgeProps {
    /// Badge text content
    pub children: Children,

    /// Badge color variant
    #[prop_or(BadgeVariant::Secondary)]
    pub variant: BadgeVariant,

    /// Additional CSS classes
    #[prop_or_default]
    pub class: Classes,

    /// Whether to show with outline style
    #[prop_or(false)]
    pub outlined: bool,

    /// Whether to show with rounded pill style
    #[prop_or(false)]
    pub pill: bool,
}

/// Badge component for displaying status indicators and labels
///
/// # Examples
///
/// ```rust
/// use yew::prelude::*;
/// use worktree_admin::components::{Badge, BadgeVariant};
///
/// #[function_component(Example)]
/// fn example() -> Html {
///     html! {
///         <Badge variant={BadgeVariant::Success}>{"Active"}</Badge>
///     }
/// }
/// ```
#[function_component(Badge)]
pub fn badge(props: &BadgeProps) -> Html {
    let mut classes = classes!("badge", props.variant.as_class(), props.class.clone());

    if props.outlined {
        classes.push("badge-outlined");
    }

    if props.pill {
        classes.push("badge-pill");
    }

    html! {
        <span class={classes}>
            { for props.children.iter() }
        </span>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_badge_variant_classes() {
        assert_eq!(BadgeVariant::Success.as_class(), "badge-success");
        assert_eq!(BadgeVariant::Warning.as_class(), "badge-warning");
        assert_eq!(BadgeVariant::Error.as_class(), "badge-error");
        assert_eq!(BadgeVariant::Secondary.as_class(), "badge-secondary");
    }
}
