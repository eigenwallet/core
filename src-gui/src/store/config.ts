import { CliMatches, getMatches } from "@tauri-apps/plugin-cli";
import { Network } from "./types";

let matches: CliMatches;
try {
  matches = await getMatches();
} catch {
  matches = {
    args: {},
    subcommand: null,
  };
}

export function getNetwork(): Network {
  if (isTestnet()) {
    return Network.Testnet;
  } else {
    return Network.Mainnet;
  }
}

export function isTestnet() {
  return matches.args.testnet?.value === true;
}
