use maud::{Markup, html};

use super::{layout_with_nav, signed_in_nav, signed_out_nav};

pub fn page(signed_in: bool) -> Markup {
    let action = if signed_in {
        signed_in_nav()
    } else {
        signed_out_nav()
    };
    layout_with_nav(
        "Frater",
        Some(action),
        html! {
            main class="home-shell" {
                h1 { "Frater" }
                p { "personal fitness tracker" }
            }
        },
    )
}
