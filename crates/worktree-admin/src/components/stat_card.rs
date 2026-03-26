//! StatCard component for displaying statistics

use yew::prelude::*;

/// Properties for the StatCard component
#[derive(Properties, PartialEq)]
pub struct StatCardProps {
    /// Icon to display (emoji or text)
    pub icon: AttrValue,
    /// The main value to display
    pub value: AttrValue,
    /// Label describing the statistic
    pub label: AttrValue,
    /// Optional trend indicator (e.g., "+12%")
    #[prop_or_default]
    pub trend: Option<AttrValue>,
    /// Optional CSS class for styling
    #[prop_or_default]
    pub class: Classes,
}

/// StatCard component - displays a statistic with icon, value, and label
#[function_component(StatCard)]
pub fn stat_card(props: &StatCardProps) -> Html {
    let classes = classes!(
        "stat-card",
        props.class.clone()
    );

    html! {
        <div class={classes}>
            <div class="stat-card-icon">
                {&props.icon}
            </div>
            <div class="stat-card-content">
                <div class="stat-card-value">
                    {&props.value}
                </div>
                <div class="stat-card-label">
                    {&props.label}
                </div>
                if let Some(trend) = &props.trend {
                    <div class="stat-card-trend">
                        {trend}
                    </div>
                }
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stat_card_props() {
        let props = StatCardProps {
            icon: "📊".into(),
            value: "1,234".into(),
            label: "Total Items".into(),
            trend: Some("+5.2%".into()),
            class: Classes::default(),
        };

        assert_eq!(props.icon, "📊");
        assert_eq!(props.value, "1,234");
        assert_eq!(props.label, "Total Items");
        assert!(props.trend.is_some());
    }
}
