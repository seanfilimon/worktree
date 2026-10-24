//! Button component with variants for Worktree Admin Panel

use yew::prelude::*;

/// Button variant types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Danger,
}

impl ButtonVariant {
    pub fn as_class(&self) -> &'static str {
        match self {
            Self::Primary => "btn-primary",
            Self::Secondary => "btn-secondary",
            Self::Danger => "btn-danger",
        }
    }
}

/// Button component properties
#[derive(Properties, PartialEq)]
pub struct ButtonProps {
    /// Button variant (default: Primary)
    #[prop_or(ButtonVariant::Primary)]
    pub variant: ButtonVariant,

    /// Button click handler
    #[prop_or_default]
    pub onclick: Callback<MouseEvent>,

    /// Whether the button is disabled
    #[prop_or(false)]
    pub disabled: bool,

    /// Button type attribute
    #[prop_or_else(|| "button".to_string())]
    pub button_type: String,

    /// Additional CSS classes
    #[prop_or_default]
    pub class: Classes,

    /// Button children (text/content)
    pub children: Children,
}

/// Button component
#[function_component(Button)]
pub fn button(props: &ButtonProps) -> Html {
    let mut classes = classes!("btn", props.variant.as_class());

    if props.disabled {
        classes.push("btn-disabled");
    }

    classes.extend(props.class.clone());

    html! {
        <button
            type={props.button_type.clone()}
            class={classes}
            onclick={props.onclick.clone()}
            disabled={props.disabled}
        >
            {props.children.clone()}
        </button>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_button_variant_classes() {
        assert_eq!(ButtonVariant::Primary.as_class(), "btn-primary");
        assert_eq!(ButtonVariant::Secondary.as_class(), "btn-secondary");
        assert_eq!(ButtonVariant::Danger.as_class(), "btn-danger");
    }
}
