use std::path::{Path, PathBuf};

use crate::defaults::{
    DEFAULT_MAX_BUY_AMOUNT, DEFAULT_MIN_BUY_AMOUNT, DEFAULT_SPREAD, default_rendezvous_points,
};
use anyhow::{Context, Result, bail};
use console::Style;
use dialoguer::Confirm;
use dialoguer::{Input, Select, theme::ColorfulTheme};
use libp2p::Multiaddr;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use url::Url;

/// Prompt user for data directory
pub fn data_directory(default_data_dir: &Path) -> Result<PathBuf> {
    let data_dir = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter data directory for asb or hit return to use default")
        .default(
            default_data_dir
                .to_str()
                .context("Unsupported characters in default path")?
                .to_string(),
        )
        .interact_text()?;

    Ok(data_dir.as_str().parse()?)
}

/// Prompt user for Bitcoin confirmation target
pub fn bitcoin_confirmation_target(default_target: u16) -> Result<u16> {
    Input::with_theme(&ColorfulTheme::default())
        .with_prompt("How fast should your Bitcoin transactions be confirmed? Your transaction fee will be calculated based on this target. Hit return to use default")
        .default(default_target)
        .interact_text()
        .map_err(Into::into)
}

/// Prompt user for listen addresses
pub fn listen_addresses(default_listen_address: &Multiaddr) -> Result<Vec<Multiaddr>> {
    print_info_box(&[
        "If you also want your maker to be reachable over other addresses (or domains)",
        "you can configure them now.",
    ]);

    let mut addresses = vec![default_listen_address.clone()];

    loop {
        let listen_address: Multiaddr = Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Enter additional multiaddress (enter to continue)")
            .allow_empty(true)
            .interact_text()?;

        if listen_address.is_empty() {
            break;
        }

        addresses.push(listen_address);
    }

    Ok(addresses)
}

/// Prompt user for electrum RPC URLs
pub fn electrum_rpc_urls(default_electrum_urls: &Vec<Url>) -> Result<Vec<Url>> {
    let mut info_lines = vec![
        "You can configure multiple Electrum servers for redundancy. At least one is required."
            .to_string(),
        "The following default Electrum RPC URLs are available. We recommend using them."
            .to_string(),
        String::new(),
    ];
    info_lines.extend(
        default_electrum_urls
            .iter()
            .enumerate()
            .map(|(i, url)| format!("{}: {}", i + 1, url)),
    );
    print_info_box(info_lines);

    // Ask if the user wants to use the default Electrum RPC URLs
    let mut electrum_rpc_urls = match Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Do you want to use the default Electrum RPC URLs?")
        .default(true)
        .interact()?
    {
        true => default_electrum_urls.clone(),
        false => Vec::new(),
    };

    let mut electrum_number = 1 + electrum_rpc_urls.len();
    let mut electrum_done = false;

    // Ask for additional electrum URLs
    while !electrum_done {
        let prompt = format!(
            "Enter additional Electrum RPC URL ({electrum_number}). Or just hit Enter to continue."
        );
        let electrum_url = Input::<String>::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .allow_empty(true)
            .interact_text()?;

        if electrum_url.is_empty() {
            electrum_done = true;
        } else if electrum_rpc_urls
            .iter()
            .any(|url| url.to_string() == electrum_url)
        {
            println!("That Electrum URL is already in the list.");
        } else {
            let electrum_url = Url::parse(&electrum_url).context("Invalid Electrum URL")?;
            electrum_rpc_urls.push(electrum_url);
            electrum_number += 1;
        }
    }

    Ok(electrum_rpc_urls)
}

/// Prompt user for Monero daemon URL
/// If the user hits enter, we will use the Monero RPC pool (None)
pub fn monero_daemon_url() -> Result<Option<Url>> {
    let type_choice = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Do you want to use the Monero RPC pool or a remote node?")
        .items(&["Use the Monero RPC pool", "Use a specific remote node"])
        .default(0)
        .interact()?;

    match type_choice {
        0 => Ok(None),
        1 => {
            let input = Input::<String>::with_theme(&ColorfulTheme::default())
                .with_prompt("Enter Monero daemon URL")
                .interact_text()?;

            Ok(Some(Url::parse(&input)?))
        }
        _ => unreachable!(),
    }
}

/// Prompt user for Tor hidden service registration
pub fn tor_hidden_service() -> Result<bool> {
    print_info_box([
        "After registering with rendezvous points, your maker needs to be reachable by takers.",
        "Running a hidden service means you'll be reachable via a .onion address",
        "- without leaking your ip address or requiring an open port.",
    ]);

    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Do you want a Tor hidden service to be created? It requires no additional setup on your end.")
        .items(&[
            "Yes, run a hidden service (recommended)",
            "No, do not run a hidden service",
        ])
        .default(0)
        .interact()?;

    Ok(selection == 0)
}

/// Prompt user for minimum Bitcoin buy amount
pub fn min_buy_amount() -> Result<bitcoin::Amount> {
    let min_buy = Input::with_theme(&ColorfulTheme::default())
        .with_prompt(
            "What's the minimum amount of Bitcoin you are willing to trade? (enter to use default)",
        )
        .default(DEFAULT_MIN_BUY_AMOUNT)
        .interact_text()?;
    bitcoin::Amount::from_btc(min_buy).map_err(Into::into)
}

/// Prompt user for maximum Bitcoin buy amount
pub fn max_buy_amount() -> Result<bitcoin::Amount> {
    let max_buy = Input::with_theme(&ColorfulTheme::default())
        .with_prompt(
            "What's the maximum amount of Bitcoin you are willing to trade? (enter to use default)",
        )
        .default(DEFAULT_MAX_BUY_AMOUNT)
        .interact_text()?;

    bitcoin::Amount::from_btc(max_buy).map_err(Into::into)
}

/// Prompt user for ask spread
pub fn ask_spread() -> Result<Decimal> {
    let ask_spread = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("What markup do you want to charge? 0.02 = 2% markup (enter to use default)")
        .default(DEFAULT_SPREAD)
        .interact_text()?;

    if !(0.0..=1.0).contains(&ask_spread) {
        bail!(format!(
            "Invalid spread {}. For the spread value floating point number in interval [0..1] are allowed.",
            ask_spread
        ))
    }

    Decimal::from_f64(ask_spread).context("Unable to parse spread")
}

/// Prompt user for rendezvous points
pub fn rendezvous_points() -> Result<Vec<Multiaddr>> {
    let default_rendezvous_points = default_rendezvous_points();

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Choice {
        ContinueWithDefaultNodes,
        AddMyOwnNodes,
        UseOnlyMyOwnNodes,
        SeeDefaultNodes,
    }

    impl TryFrom<usize> for Choice {
        type Error = anyhow::Error;

        fn try_from(value: usize) -> std::result::Result<Self, Self::Error> {
            Ok(match value {
                0 => Choice::ContinueWithDefaultNodes,
                1 => Choice::AddMyOwnNodes,
                2 => Choice::UseOnlyMyOwnNodes,
                3 => Choice::SeeDefaultNodes,
                _ => bail!("unknown choice"),
            })
        }
    }

    let theme = ColorfulTheme::default();
    print_info_box(&[
        "For takers to trade with your maker, it needs to be discovered first.",
        "This happens at 'rendezvous points', which are community run.",
        "You can now choose with which of those nodes to connect.",
    ]);
    let input = Select::with_theme(&theme)
        .with_prompt("How do you want to procede?")
        .items(&[
            "Connect to default rendezvous points (recommended)",
            "Connect to default rendezvous points and also specify my own",
            "Connect only to my own rendezvous point(s) (not recommended)",
            "Print a list of the default rendezvous points",
        ])
        .default(0);

    let mut choice: Choice = input.clone().interact()?.try_into()?;

    while choice == Choice::SeeDefaultNodes {
        print_info_box(default_rendezvous_points.iter().map(|i| format!("{i}")));
        choice = input.clone().interact()?.try_into()?;
    }

    let mut rendezvous_points = match choice {
        Choice::AddMyOwnNodes | Choice::ContinueWithDefaultNodes => default_rendezvous_points,
        _ => Vec::new(),
    };

    while matches!(choice, Choice::AddMyOwnNodes | Choice::UseOnlyMyOwnNodes) {
        let address: Multiaddr = Input::with_theme(&theme)
            .with_prompt("Enter an address of your rendezvous point (enter to continue)")
            .allow_empty(true)
            .interact_text()?;

        if address.is_empty() {
            if rendezvous_points.is_empty() {
                print_info_box(&[
                    "You currently have zero rendezvous points configured.",
                    "Your maker will not be reachable and not make any swaps if you continue.",
                ]);
                let choice = Confirm::with_theme(&theme)
                    .with_prompt("Do you wish to continue, even with your maker unreachable?")
                    .default(false)
                    .interact()?;
                if !choice {
                    println!("Good choice. Aborting now, so you can restart");
                    bail!("No rendezvous points configured");
                }
            }
            break;
        }

        rendezvous_points.push(address);
    }

    Ok(rendezvous_points)
}

pub fn developer_tip() -> Result<Decimal> {
    // We first ask if the user wants to enable developer tipping at all
    // We do not select a default here as to not bias the user
    //
    // If not, we return 0
    // If yes, we ask for the percentage and default to 1% (0.01)
    let lines = [
        "This project has been developed by a small team of volunteers since 2022",
        "We rely on donations and the Monero CCS to continue our efforts.",
        "",
        "You can choose to donate a small part of each swap toward development.",
        "",
        "Donations will be used for Github bounties among other things.",
        "",
        "The tip is sent as an additional output of the Monero lock transaction.",
        "It does not require an extra transaction and you remain fully private.",
        "",
        "If enabled, you'll enter the percentage in the next step.",
    ];
    print_info_box(lines);

    let enable_developer_tip = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Do you want to enable developer tipping?")
        .interact()?;

    if !enable_developer_tip {
        return Ok(Decimal::ZERO);
    }

    let developer_tip = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter developer tip percentage (value between 0.00 and 1.00; 0.01 means 1% of the swap amount is donated)")
        .default(Decimal::from_f64(0.01).unwrap())
        .interact_text()?;

    if !(Decimal::ZERO..=Decimal::ONE).contains(&developer_tip) {
        bail!(format!(
            "Invalid developer tip {}. For the developer tip value floating point number in interval [0..1] are allowed.",
            developer_tip
        ))
    }

    let developer_tip_percentage =
        developer_tip.saturating_mul(Decimal::from_u64(100).expect("100 to fit in u64"));

    print_info_box([&format!(
        "You will tip {}% of each swap to the developers. Thank you for your support!",
        developer_tip_percentage
    )]);

    Ok(developer_tip)
}

/// Print a boxed info message using console styling to match dialoguer output
pub fn print_info_box<L, S>(lines: L)
where
    L: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let terminal_width = terminal_size::terminal_size().map_or(200, |(width, _)| width.0 as usize);

    let border = Style::new().cyan();
    let content = Style::new().bold();

    let mut collected: Vec<String> = lines.into_iter().map(|s| s.as_ref().to_string()).collect();

    if collected.is_empty() {
        collected.push(String::new());
    }

    let content_width = collected
        .iter()
        .map(|s| s.len())
        .max()
        .expect("Failed to get line width");
    let line_width = (content_width + 2).min(terminal_width);

    let top = format!("┌{}", "─".repeat(line_width.saturating_sub(1)));
    let bottom = format!("└{}", "─".repeat(line_width.saturating_sub(1)));
    println!("");
    println!("{}", border.apply_to(&top));
    for l in collected {
        println!("{} {}", border.apply_to("│"), content.apply_to(l));
    }
    println!("{}", border.apply_to(&bottom));
}
