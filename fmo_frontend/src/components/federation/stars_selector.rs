use leptos::prelude::*;

#[component]
pub fn StarsSelector(default_value: u8, selected_stars: WriteSignal<u8>) -> impl IntoView {
    selected_stars.set(default_value);

    let (selected, set_selected) = signal(default_value);
    let (hover, set_hover) = signal(None);

    let star = move |star_idx: u8, fill: bool, border: bool| {
        view! {
            <svg
                class={
                    if fill {
                        "w-6 h-6 ms-2 text-yellow-300"
                    } else {
                        "w-6 h-6 ms-2 text-gray-300 dark:text-gray-500"
                    }
                }
                aria-hidden="true"
                xmlns="http://www.w3.org/2000/svg"
                fill="currentColor"
                stroke=border.then_some("orange")
                stroke-width=border.then_some("2px")
                viewBox="0 0 22 20"
                on:mouseenter=move |_| {set_hover.set(Some(star_idx));}
                on:click=move |_| {
                    set_selected.set(star_idx);
                    selected_stars.set(star_idx);
                }
                on:mouseleave=move |_| {set_hover.set(None);}
            >
                <path d="M20.924 7.625a1.523 1.523 0 0 0-1.238-1.044l-5.051-.734-2.259-4.577a1.534 1.534 0 0 0-2.752 0L7.365 5.847l-5.051.734A1.535 1.535 0 0 0 1.463 9.2l3.656 3.563-.863 5.031a1.532 1.532 0 0 0 2.226 1.616L11 17.033l4.518 2.375a1.534 1.534 0 0 0 2.226-1.617l-.863-5.03L20.537 9.2a1.523 1.523 0 0 0 .387-1.575Z"/>
            </svg>
        }
    };

    view! {
        <div
            class="inline-flex px-1 py-2"
            on:mouseleave=move |_| {set_hover.set(None);}
        >
            { move || {
                let selected = selected.get();
                let hover = hover.get();
                (1..=5).map(|idx| star(idx, selected >= idx, hover.is_some_and(|h| h >= idx))).collect::<Vec<_>>()
            }}
        </div>
    }
}
