use std::path::{Path, PathBuf};

use crate::defaults::{
    DEFAULT_MAX_BUY_AMOUNT, DEFAULT_MIN_BUY_AMOUNT, DEFAULT_SPREAD, default_rendezvous_points,
};
use anyhow::{Context, Result, bail};
use dialoguer::Confirm;
use dialoguer::{Input, Select, theme::ColorfulTheme};
use libp2p::Multiaddr;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use url::Url;
use console::{Style};

/// Print a boxed info message using console styling to match dialoguer output
pub fn print_info_box<L, S>(lines: L)
where
    L: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let terminal_width = terminal_size::terminal_size().map_or(200, |(width, _)| width.0 as usize);

    let border = Style::new().cyan();
    let content = Style::new().bold();

    let mut collected: Vec<String> = lines
        .into_iter()
        .map(|s| s.as_ref().to_string())
        .collect();

    if collected.is_empty() {
        collected.push(String::new());
    }

    let content_width = collected.iter().map(|s| s.len()).max().expect("Failed to get line width");
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

/// Print a single-line note with a subtle, consistent style
// Removed print_note: prefer using print_info_box exclusively for consistent styling

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
    let listen_addresses = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter multiaddresses (comma separated) on which asb should list for peer-to-peer communications or hit return to use default")
        .default(format!("{}", default_listen_address))
        .interact_text()?;

    listen_addresses
        .split(',')
        .map(|str| str.parse())
        .collect::<Result<Vec<Multiaddr>, _>>()
        .map_err(Into::into)
}

/// Prompt user for electrum RPC URLs
pub fn electrum_rpc_urls(default_electrum_urls: &Vec<Url>) -> Result<Vec<Url>> {
    let mut info_lines = vec![
        "You can configure multiple Electrum servers for redundancy. At least one is required.".to_string(),
        "The following default Electrum RPC URLs are available. We recommend using them.".to_string(),
        String::new(),
    ];
    for (i, url) in default_electrum_urls.iter().enumerate() {
        info_lines.push(format!("{}: {}", i + 1, url));
    }
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
            print_info_box(["That Electrum URL is already in the list."]);
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
        .items(&["Use the Monero RPC pool", "Use a remote node"])
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
        "Your ASB needs to be reachable from the outside world to provide quotes to takers.",
        "Your ASB can run a hidden service for itself. It'll be reachable at an .onion address.",
        "You do not have to run a Tor daemon yourself. You do not have to manage anything.",
        "This will hide your IP address and allow you to run from behind a firewall without opening ports.",
    ]);

    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Do you want a Tor hidden service to be created?")
        .items(&[
            "Yes, run a hidden service",
            "No, do not run a hidden service",
        ])
        .default(0)
        .interact()?;

    Ok(selection == 0)
}

/// Prompt user for minimum Bitcoin buy amount
pub fn min_buy_amount() -> Result<bitcoin::Amount> {
    let min_buy = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter minimum Bitcoin amount you are willing to accept per swap or hit enter to use default.")
        .default(DEFAULT_MIN_BUY_AMOUNT)
        .interact_text()?;
    bitcoin::Amount::from_btc(min_buy).map_err(Into::into)
}

/// Prompt user for maximum Bitcoin buy amount
pub fn max_buy_amount() -> Result<bitcoin::Amount> {
    let max_buy = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter maximum Bitcoin amount you are willing to accept per swap or hit enter to use default.")
        .default(DEFAULT_MAX_BUY_AMOUNT)
        .interact_text()?;
    bitcoin::Amount::from_btc(max_buy).map_err(Into::into)
}

/// Prompt user for ask spread
pub fn ask_spread() -> Result<Decimal> {
    let ask_spread = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Enter spread (in percent; value between 0.x and 1.0) to be used on top of the market rate or hit enter to use default.")
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
    let mut info_lines = vec![
        "Your ASB can register with multiple rendezvous nodes for discoverability.".to_string(),
        "They act as sort of bootstrap nodes for peer discovery within the peer-to-peer network.".to_string(),
        String::new(),
        "The following rendezvous points are ran by community members. We recommend using them.".to_string(),
        String::new(),
    ];
    for (i, point) in default_rendezvous_points.iter().enumerate() {
        info_lines.push(format!("{}: {}", i + 1, point));
    }
    print_info_box(info_lines);

    // Ask if the user wants to use the default rendezvous points
    let use_default_rendezvous_points = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Do you want to use the default rendezvous points? (y/n)")
        .items(&[
            "Use default rendezvous points",
            "Do not use default rendezvous points",
        ])
        .default(0)
        .interact()?;

    let mut rendezvous_points = match use_default_rendezvous_points {
        0 => {
            print_info_box(["You can now configure additional rendezvous points."]);
            default_rendezvous_points
        }
        _ => Vec::new(),
    };

    let mut number = 1 + rendezvous_points.len();
    let mut done = false;

    while !done {
        let prompt = format!(
            "Enter the address for rendezvous node ({number}). Or just hit Enter to continue."
        );
        let rendezvous_addr = Input::<Multiaddr>::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .allow_empty(true)
            .interact_text()?;

        if rendezvous_addr.is_empty() {
            done = true;
        } else if rendezvous_points.contains(&rendezvous_addr) {
            print_info_box(["That rendezvous address is already in the list."]);
        } else {
            rendezvous_points.push(rendezvous_addr);
            number += 1;
        }
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
        "This project is developed by a small team of volunteers since 2022",
        "We rely on donations and the Monero CCS to continue our efforts.",
        "",
        "You can choose to donate a small part of each swap toward development.",
        "",
        "Donations are used for Github bounties and for paying for infrastructure costs.",
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
        .with_prompt("Enter developer tip percentage (in percent; value between 0.x and 1.0; 0.01 means 1% of the swap amount is donated)")
        .default(Decimal::from_f64(0.01).unwrap())
        .interact_text()?;

    let developer_tip_percentage = developer_tip.saturating_mul(Decimal::from_u64(100).expect("100 to fit in u64"));
    print_info_box([&format!(
        "You will tip {}% of each swap to the developers. Thank you for your support!",
        developer_tip_percentage
    )]);

    Ok(developer_tip)
}