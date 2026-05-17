use std::collections::BTreeMap;

use fedimint_core::config::FederationId;
use fedimint_core::util::backoff_util::background_backoff;
use fedimint_core::util::retry;
use fedimint_core::{NumPeers, PeerId};
use fmo_api_types::GuardianHealth;
use leptos::prelude::*;

use crate::components::badge::{Badge, BadgeLevel};
use crate::BASE_URL;

#[component]
pub fn Guardians(federation_id: FederationId, guardians: Vec<Guardian>) -> impl IntoView {
    let n = guardians.len();
    let t = NumPeers::from(n).threshold();

    let health_resource =
        LocalResource::new(move || async move { fetch_guardian_health(federation_id).await });

    let warn_if_true = |warn| {
        if warn {
            BadgeLevel::Warning
        } else {
            BadgeLevel::Success
        }
    };

    let guardians = guardians
        .into_iter()
        .enumerate()
        .map(|(guardian_idx, guardian)| {
            view! {
                <li class="py-3 sm:py-4">
                    <div class="flex items-center">
                        <div class="flex-1 min-w-0 ms-4">
                            <p class="text-sm font-medium text-gray-900 truncate dark:text-white">
                                {guardian.name}
                            </p>
                            <p class="text-sm text-gray-500 truncate dark:text-gray-400">
                                {guardian.url}
                            </p>
                            <p>
                                { move || match health_resource.get() {
                                    Some(health) => {
                                        let health = health.get(&PeerId::from(guardian_idx as u16)).expect("Guardian exists").clone();

                                        let mut badges = vec![];
                                        let software_version = health
                                            .software_version
                                            .unwrap_or_else(|| "Version unknown".to_owned());
                                        if let Some(latest) = health.latest {
                                            badges.push(view! {
                                                <Badge level=BadgeLevel::Success>
                                                    Online
                                                </Badge>
                                            }.into_view());

                                            badges.push(view! {
                                                <Badge
                                                    level=warn_if_true(latest.session_outdated)
                                                    tooltip=latest.session_outdated.then_some("Guardian is lacking behind others".to_owned())
                                                >
                                                    {format!("Session {}", latest.session_count)}
                                                </Badge>
                                            }.into_view());

                                            badges.push(view! {
                                                <Badge
                                                    level=warn_if_true(latest.block_outdated)
                                                    tooltip=latest.block_outdated.then_some("Guardian's bitcoind is out of sync".to_owned())
                                                >
                                                    {format!("Block {}", latest.block_height - 1)}
                                                </Badge>
                                            }.into_view());
                                        } else {
                                            badges.push(view! {
                                                <Badge level=BadgeLevel::Error>
                                                    Offline
                                                </Badge>
                                            }.into_view());
                                        }
                                        badges.push(view! {
                                            <Badge level=BadgeLevel::Info>
                                                {software_version}
                                            </Badge>
                                        }.into_view());

                                        view! { {badges} }.into_any()
                                    }
                                    None => {
                                        view! {
                                            <span class="text-sm font-medium text-gray-500 dark:text-gray-400">
                                                "Loading"
                                            </span>
                                        }.into_any()
                                    }
                                }}
                            </p>
                        </div>
                    </div>
                </li>
            }
        })
        .collect::<Vec<_>>();

    view! {
        <div class="w-full h-full p-4 bg-white border border-gray-200 rounded-lg shadow sm:p-8 dark:bg-gray-800 dark:border-gray-700">
            <div class="flex items-center justify-between mb-4">
                <h5 class="text-xl font-bold leading-none text-gray-900 dark:text-white">
                    Guardians
                </h5>
                <p class="text-sm font-medium text-gray-500 dark:text-gray-400">
                    {format!("{} of {} Federation", t, n)}
                </p>
            </div>
            <div class="flow-root">
                <ul role="list" class="divide-y divide-gray-200 dark:divide-gray-700">
                    {guardians}
                </ul>
            </div>
        </div>
    }
}

pub struct Guardian {
    pub name: String,
    pub url: String,
}

async fn fetch_guardian_health(id: FederationId) -> BTreeMap<PeerId, GuardianHealth> {
    retry(
        "fetching guardian health",
        background_backoff(),
        || async move {
            reqwest::get(format!("{}/federations/{}/health", BASE_URL, id))
                .await?
                .json::<BTreeMap<PeerId, GuardianHealth>>()
                .await
                .map_err(Into::into)
        },
    )
    .await
    .expect("Will never return Err")
}
