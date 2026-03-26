//! Card component - Generic container with title and children

use yew::prelude::*;

/// Properties for the Card component
#[derive(Properties, PartialEq)]
pub struct CardProps {
    /// Card title (optional)
    #[prop_or_default]
    pub title: Option<AttrValue>,

    /// Additional CSS classes
    #[prop_or_default]
    pub class: Classes,

    /// Card content
    pub children: Children,

    /// Optional header actions/buttons
    #[prop_or_default]
    pub header_actions: Option<Html>,
}

/// A generic card container component
///
/// # Example
/// ```rust
/// use yew::prelude::*;
/// use worktree_admin::components::Card;
///
/// #[function_component(Example)]
/// fn example() -> Html {
///     html! {
///         <Card title="My Card">
///             <p>{"Card content goes here"}</p>
///         </Card>
///     }
/// }
/// ```
#[function_component(Card)]
pub fn card(props: &CardProps) -> Html {
    let card_classes = classes!("card", props.class.clone());

    html! {
        <div class={card_classes}>
            if let Some(title) = &props.title {
                <div class="card-header">
                    <h3 class="card-title">{title}</h3>
                    if let Some(actions) = &props.header_actions {
                        <div class="card-actions">
                            {actions.clone()}
                        </div>
                    }
                </div>
            }
            <div class="card-body">
                {props.children.clone()}
            </div>
        </div>
    }
}

/// Properties for the CardFooter component
#[derive(Properties, PartialEq)]
pub struct CardFooterProps {
    /// Footer content
    pub children: Children,

    /// Additional CSS classes
    #[prop_or_default]
    pub class: Classes,
}

/// Optional card footer component
#[function_component(CardFooter)]
pub fn card_footer(props: &CardFooterProps) -> Html {
    let footer_classes = classes!("card-footer", props.class.clone());

    html! {
        <div class={footer_classes}>
            {props.children.clone()}
        </div>
    }
}
