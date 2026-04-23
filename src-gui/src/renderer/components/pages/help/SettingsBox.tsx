import {
  Alert,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableRow,
  Typography,
  IconButton,
  Box,
  Tooltip,
  Select,
  MenuItem,
  TableHead,
  Paper,
  Button,
  Dialog,
  DialogContent,
  DialogActions,
  DialogTitle,
  useTheme,
  Switch,
  SelectChangeEvent,
  TextField,
  ToggleButton,
  ToggleButtonGroup,
} from "@mui/material";
import {
  addNode,
  addRendezvousPoint,
  FiatCurrency,
  moveUpNode,
  removeNode,
  removeRendezvousPoint,
  resetSettings,
  setFetchFiatPrices,
  setFiatCurrency,
  setTheme,
  setNetworkProxyMode,
  setTorSocksAddress,
  setEnableMoneroTor,
  setAllowDfxClearnet,
  setUseMoneroRpcPool,
  setMoneroRedeemPolicy,
  setMoneroRedeemAddress,
  setBitcoinRefundAddress,
  setBitcoinRefundPolicy,
  RedeemPolicy,
  RefundPolicy,
  NetworkProxyMode,
} from "store/features/settingsSlice";
import { Blockchain, Network } from "store/types";
import { useAppDispatch, useNodes, useSettings } from "store/hooks";
import ValidatedTextField from "renderer/components/other/ValidatedTextField";
import HelpIcon from "@mui/icons-material/HelpOutline";
import { ReactNode, useEffect, useState } from "react";
import { Theme } from "renderer/components/theme";
import {
  Add,
  ArrowUpward,
  Delete,
  Edit,
  HourglassEmpty,
  Refresh,
} from "@mui/icons-material";
import { invoke as invokeUnsafe } from "@tauri-apps/api/core";

import { getNetwork } from "store/config";
import { currencySymbol } from "utils/formatUtils";
import InfoBox from "renderer/components/pages/swap/swap/components/InfoBox";
import { isValidMultiAddressWithPeerId } from "utils/parseUtils";
import { getNodeStatus } from "renderer/rpc";
import { setStatus } from "store/features/nodesSlice";
import MoneroAddressTextField from "renderer/components/inputs/MoneroAddressTextField";
import BitcoinAddressTextField from "renderer/components/inputs/BitcoinAddressTextField";
import DonationTipDialog, {
  formatDonationTipLabel,
} from "renderer/components/modal/donation-tip/DonationTipDialog";

const PLACEHOLDER_ELECTRUM_RPC_URL = "ssl://blockstream.info:700";
const PLACEHOLDER_MONERO_NODE_URL = "http://xmr-node.cakewallet.com:18081";

/** Returns true when the IP part of an "ip:port" string is the IPv4 loopback. */
const isSocksAddressLocalhost = (val: string): boolean => {
  const ip = val.slice(0, val.lastIndexOf(":"));
  return ip === "127.0.0.1";
};

/**
 * Result of parsing a user-entered SOCKS5 address as it's being typed.
 *
 * The phases track how far the user has progressed through `ipv4:port`:
 * `empty` → `partialIp` → `ipComplete` → `awaitingPort` → `complete`, with
 * `invalid` for anything that cannot be fixed by typing more characters
 * (e.g. octet > 255, leading zero, port > 65535).
 *
 * Only `complete` triggers a SOCKS5 probe. Other phases drive the
 * helper-text / error UI so the user sees *why* input is incomplete.
 */
type SocksAddressParse =
  | { phase: "empty" }
  | { phase: "partialIp"; hint: string }
  | { phase: "ipComplete"; hint: string }
  | { phase: "awaitingPort"; hint: string }
  | { phase: "complete" }
  | { phase: "invalid"; error: string };

/**
 * Validates a single IPv4 octet as the user types it.
 *
 * `partial` means "so far so good, may accept more digits" (e.g. "1", "25").
 * `complete` means the octet is fully specified (e.g. "192", "0", "255").
 * Rejects leading zeros ("01", "001") to match Rust's `SocketAddrV4::from_str`.
 */
const classifyOctet = (
  s: string,
): { kind: "empty" } | { kind: "partial" } | { kind: "complete" } | { kind: "invalid" } => {
  if (s === "") return { kind: "empty" };
  if (!/^\d+$/.test(s)) return { kind: "invalid" };
  if (s.length > 3) return { kind: "invalid" };
  if (s.length > 1 && s.startsWith("0")) return { kind: "invalid" };
  const n = parseInt(s, 10);
  if (n > 255) return { kind: "invalid" };
  // An octet of 1–2 digits could still grow (e.g. "2" → "25" → "255"),
  // so we call those "partial"; 3 digits are final.
  if (s.length === 3) return { kind: "complete" };
  return { kind: "partial" };
};

/**
 * Parse user input through each phase of a `SocketAddrV4`. Strictly matches
 * Rust's `SocketAddrV4::from_str` so the frontend and backend never disagree:
 * rejects leading zeros, octets > 255, and ports outside 1–65535.
 */
const parseSocks5Address = (raw: string): SocksAddressParse => {
  if (raw === "") return { phase: "empty" };

  const colonCount = (raw.match(/:/g) ?? []).length;
  if (colonCount > 1) {
    return { phase: "invalid", error: "Only one ':' is allowed — use ipv4:port" };
  }

  const [ipPart, portPart] = raw.includes(":") ? raw.split(":") : [raw, null];

  const octets = ipPart.split(".");
  if (octets.length > 4) {
    return { phase: "invalid", error: "IPv4 has exactly 4 octets" };
  }

  // Classify each octet that's been typed so far.
  let allOctetsComplete = true;
  for (let i = 0; i < octets.length; i++) {
    const result = classifyOctet(octets[i]);
    if (result.kind === "invalid") {
      return {
        phase: "invalid",
        error: `Invalid octet "${octets[i]}" — each octet must be 0–255 with no leading zeros`,
      };
    }
    // Non-last octets must be complete (otherwise the dot is premature).
    if (i < octets.length - 1 && result.kind !== "complete" && result.kind !== "partial") {
      return { phase: "invalid", error: "Each '.' must follow an octet" };
    }
    // `partial` (1-2 digits) is already a valid octet numerically; only an
    // empty trailing octet (e.g. "127.0.0.") means the IP is unfinished.
    if (result.kind === "empty") allOctetsComplete = false;
  }

  const hasAllFourOctets = octets.length === 4;

  if (!hasAllFourOctets || !allOctetsComplete) {
    // Still building up the IP.
    if (portPart !== null) {
      return {
        phase: "invalid",
        error: "Finish the IPv4 address before adding ':port'",
      };
    }
    return {
      phase: "partialIp",
      hint: hasAllFourOctets
        ? "Finish the last octet"
        : `Enter ${4 - octets.length} more octet${octets.length === 3 ? "" : "s"}`,
    };
  }

  // IP is complete. Now handle the port side.
  if (portPart === null) {
    return { phase: "ipComplete", hint: "Type ':' then the port" };
  }
  if (portPart === "") {
    return { phase: "awaitingPort", hint: "Enter the port (1–65535)" };
  }
  if (!/^\d+$/.test(portPart)) {
    return { phase: "invalid", error: "Port must be digits only" };
  }
  if (portPart.length > 1 && portPart.startsWith("0")) {
    return { phase: "invalid", error: "Port must not have leading zeros" };
  }
  const port = parseInt(portPart, 10);
  if (port < 1 || port > 65535) {
    return { phase: "invalid", error: "Port must be between 1 and 65535" };
  }
  if (portPart.length > 5) {
    return { phase: "invalid", error: "Port must be at most 5 digits" };
  }
  return { phase: "complete" };
};

/** True once the string is a fully formed ipv4:port ready for a probe. */
const isValidSocksAddress = (val: string): boolean =>
  parseSocks5Address(val).phase === "complete";

/**
 * Runs a single SOCKS5 handshake probe via the `check_socks5_address` Tauri
 * command. Failures (parse error, TCP error, unexpected handshake response)
 * all collapse into `false` — callers only need to know reachable/not.
 */
async function probeSocks5(address: string): Promise<boolean> {
  try {
    return await invokeUnsafe<boolean>("check_socks5_address", { address });
  } catch {
    return false;
  }
}

/**
 * The settings box, containing the settings for the GUI.
 */
export default function SettingsBox() {
  const theme = useTheme();

  return (
    <InfoBox
      title={
        <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
          Settings
        </Box>
      }
      mainContent={
        <Typography variant="subtitle2">
          Customize the settings of the GUI. Some of these require a restart to
          take effect.
        </Typography>
      }
      additionalContent={
        <>
          {/* Table containing the settings */}
          <TableContainer>
            <Table>
              <TableBody>
                <NetworkProxySetting />
                <DonationTipSetting />
                <RedeemPolicySetting />
                <RefundPolicySetting />
                <ElectrumRpcUrlSetting />
                <MoneroRpcPoolSetting />
                <MoneroNodeUrlSetting />
                <FetchFiatPricesSetting />
                <ThemeSetting />
                <RendezvousPointsSetting />
              </TableBody>
            </Table>
          </TableContainer>
          {/* Reset button with a bit of spacing */}
          <Box
            sx={(theme) => ({
              mt: theme.spacing(2),
            })}
          />
          <ResetButton />
        </>
      }
      icon={null}
      loading={false}
    />
  );
}

/**
 * A button that allows you to reset the settings.
 * Opens a modal that asks for confirmation first.
 */
function ResetButton() {
  const dispatch = useAppDispatch();
  const [modalOpen, setModalOpen] = useState(false);

  const onReset = () => {
    dispatch(resetSettings());
    setModalOpen(false);
  };

  return (
    <>
      <Button variant="outlined" onClick={() => setModalOpen(true)}>
        Reset Settings
      </Button>
      <Dialog open={modalOpen} onClose={() => setModalOpen(false)}>
        <DialogTitle>Reset Settings</DialogTitle>
        <DialogContent>
          Are you sure you want to reset the settings?
        </DialogContent>
        <DialogActions>
          <Button onClick={() => setModalOpen(false)}>Cancel</Button>
          <Button color="primary" onClick={onReset}>
            Reset
          </Button>
        </DialogActions>
      </Dialog>
    </>
  );
}

/**
 * A setting that allows you to enable or disable the fetching of fiat prices.
 */
function FetchFiatPricesSetting() {
  const fetchFiatPrices = useSettings((s) => s.fetchFiatPrices);
  const dispatch = useAppDispatch();

  return (
    <>
      <TableRow>
        <TableCell>
          <SettingLabel
            label="Query fiat prices"
            tooltip="Whether to fetch fiat prices via the clearnet. This is required for the price display to work. If you require total anonymity and don't use a VPN, you should disable this."
          />
        </TableCell>
        <TableCell>
          <Switch
            color="primary"
            checked={fetchFiatPrices}
            onChange={(event) =>
              dispatch(setFetchFiatPrices(event.currentTarget.checked))
            }
          />
        </TableCell>
      </TableRow>
      {fetchFiatPrices ? <FiatCurrencySetting /> : <></>}
    </>
  );
}

/**
 * A setting that allows you to select the fiat currency to display prices in.
 */
function FiatCurrencySetting() {
  const fiatCurrency = useSettings((s) => s.fiatCurrency);
  const dispatch = useAppDispatch();
  const onChange = (e: SelectChangeEvent<FiatCurrency>) =>
    dispatch(setFiatCurrency(e.target.value as FiatCurrency));

  return (
    <TableRow>
      <TableCell>
        <SettingLabel
          label="Fiat currency"
          tooltip="This is the currency that the price display will show prices in."
        />
      </TableCell>
      <TableCell>
        <Select
          value={fiatCurrency}
          onChange={onChange}
          variant="outlined"
          fullWidth
        >
          {Object.values(FiatCurrency).map((currency) => (
            <MenuItem key={currency} value={currency}>
              <Box
                sx={{
                  display: "flex",
                  justifyContent: "space-between",
                  width: "100%",
                }}
              >
                <Box>{currency}</Box>
                <Box>{currencySymbol(currency)}</Box>
              </Box>
            </MenuItem>
          ))}
        </Select>
      </TableCell>
    </TableRow>
  );
}

/**
 * URL validation function, forces the URL to be in the format of "protocol://host:port/"
 */
function isValidUrl(url: string, allowedProtocols: string[]): boolean {
  const urlPattern = new RegExp(
    `^(${allowedProtocols.join("|")})://[^\\s]+:\\d+/?$`,
  );
  return urlPattern.test(url);
}

/**
 * A setting that allows you to select the Electrum RPC URL to use.
 */
function ElectrumRpcUrlSetting() {
  const [tableVisible, setTableVisible] = useState(false);
  const network = getNetwork();

  const isValid = (url: string) => isValidUrl(url, ["ssl", "tcp"]);

  return (
    <TableRow>
      <TableCell>
        <SettingLabel
          label="Custom Electrum RPC URL"
          tooltip="This is the URL of the Electrum server that the GUI will connect to. It is used to sync Bitcoin transactions. If you leave this field empty, the GUI will choose from a list of known servers at random."
        />
      </TableCell>
      <TableCell>
        <IconButton onClick={() => setTableVisible(true)} size="large">
          {<Edit />}
        </IconButton>
        {tableVisible ? (
          <NodeTableModal
            open={tableVisible}
            onClose={() => setTableVisible(false)}
            network={network}
            blockchain={Blockchain.Bitcoin}
            isValid={isValid}
            placeholder={PLACEHOLDER_ELECTRUM_RPC_URL}
          />
        ) : (
          <></>
        )}
      </TableCell>
    </TableRow>
  );
}

/**
 * A label for a setting, with a tooltip icon.
 */
function SettingLabel({
  label,
  tooltip,
  disabled = false,
}: {
  label: ReactNode;
  tooltip: string | null;
  disabled?: boolean;
}) {
  const opacity = disabled ? 0.5 : 1;

  return (
    <Box
      style={{ display: "flex", alignItems: "center", gap: "0.5rem", opacity }}
    >
      <Box>{label}</Box>
      <Tooltip title={tooltip}>
        <IconButton size="small" disabled={disabled}>
          <HelpIcon />
        </IconButton>
      </Tooltip>
    </Box>
  );
}

/**
 * A setting that allows you to toggle between using the Monero RPC Pool and custom nodes.
 */
function MoneroRpcPoolSetting() {
  const useMoneroRpcPool = useSettings((s) => s.useMoneroRpcPool);
  const dispatch = useAppDispatch();

  const handleChange = (
    event: React.MouseEvent<HTMLElement>,
    newValue: string,
  ) => {
    if (newValue !== null) {
      dispatch(setUseMoneroRpcPool(newValue === "pool"));
    }
  };

  return (
    <TableRow>
      <TableCell>
        <SettingLabel
          label="Monero Node Selection"
          tooltip="Choose between using a load-balanced pool of Monero nodes for better reliability, or configure custom Monero nodes."
        />
      </TableCell>
      <TableCell>
        <ToggleButtonGroup
          color="primary"
          value={useMoneroRpcPool ? "pool" : "custom"}
          exclusive
          onChange={handleChange}
          aria-label="Monero node selection"
          size="small"
        >
          <ToggleButton value="pool">Pool (Recommended)</ToggleButton>
          <ToggleButton value="custom">Manual</ToggleButton>
        </ToggleButtonGroup>
      </TableCell>
    </TableRow>
  );
}

/**
 * A setting that allows you to configure a single Monero Node URL.
 * Gets disabled when RPC pool is enabled.
 */
function MoneroNodeUrlSetting() {
  const network = getNetwork();
  const useMoneroRpcPool = useSettings((s) => s.useMoneroRpcPool);
  const moneroNodeUrl = useSettings(
    (s) => s.nodes[network][Blockchain.Monero][0] || "",
  );
  const nodeStatuses = useNodes((s) => s.nodes);
  const dispatch = useAppDispatch();
  const [isRefreshing, setIsRefreshing] = useState(false);

  const currentNodes = useSettings((s) => s.nodes[network][Blockchain.Monero]);

  const handleNodeUrlChange = (newUrl: string) => {
    // Remove existing nodes and add the new one
    currentNodes.forEach((node) => {
      dispatch(removeNode({ network, type: Blockchain.Monero, node }));
    });

    if (newUrl.trim()) {
      dispatch(
        addNode({ network, type: Blockchain.Monero, node: newUrl.trim() }),
      );
    }
  };

  const handleRefreshStatus = async () => {
    // Don't refresh if pool is enabled or no node URL is configured
    if (!moneroNodeUrl || useMoneroRpcPool) return;

    setIsRefreshing(true);
    try {
      const status = await getNodeStatus(
        moneroNodeUrl,
        Blockchain.Monero,
        network,
      );

      // Update the status in the store
      dispatch(
        setStatus({
          node: moneroNodeUrl,
          status,
          blockchain: Blockchain.Monero,
        }),
      );
    } catch (error) {
      console.error("Failed to refresh node status:", error);
    } finally {
      setIsRefreshing(false);
    }
  };

  const isValid = (url: string) => url === "" || isValidUrl(url, ["http"]);
  const nodeStatus = moneroNodeUrl
    ? nodeStatuses[Blockchain.Monero][moneroNodeUrl]
    : null;

  return (
    <TableRow>
      <TableCell>
        <SettingLabel
          label="Custom Monero Node URL"
          tooltip={
            useMoneroRpcPool
              ? "This setting is disabled because Monero RPC pool is enabled. Disable the RPC pool to configure a custom node."
              : "This is the URL of the Monero node that the GUI will connect to. It is used to sync Monero transactions. If you leave this field empty, the GUI will choose from a list of known servers at random."
          }
          disabled={useMoneroRpcPool}
        />
      </TableCell>
      <TableCell>
        <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
          <ValidatedTextField
            value={moneroNodeUrl}
            onValidatedChange={(value) => value && handleNodeUrlChange(value)}
            placeholder={PLACEHOLDER_MONERO_NODE_URL}
            disabled={useMoneroRpcPool}
            fullWidth
            isValid={isValid}
            variant="outlined"
            noErrorWhenEmpty
          />
          <>
            <Tooltip
              title={
                useMoneroRpcPool
                  ? "Node status checking is disabled when using the pool"
                  : !moneroNodeUrl
                    ? "Enter a node URL to check status"
                    : "Node status"
              }
            >
              <Box sx={{ display: "flex", alignItems: "center" }}>
                <Circle
                  color={
                    useMoneroRpcPool || !moneroNodeUrl
                      ? "gray"
                      : nodeStatus
                        ? "green"
                        : "red"
                  }
                />
              </Box>
            </Tooltip>
            <Tooltip
              title={
                useMoneroRpcPool
                  ? "Node status refresh is disabled when using the pool"
                  : !moneroNodeUrl
                    ? "Enter a node URL to refresh status"
                    : "Refresh node status"
              }
            >
              <IconButton
                onClick={handleRefreshStatus}
                disabled={isRefreshing || useMoneroRpcPool || !moneroNodeUrl}
                size="small"
              >
                {isRefreshing ? <HourglassEmpty /> : <Refresh />}
              </IconButton>
            </Tooltip>
          </>
        </Box>
      </TableCell>
    </TableRow>
  );
}

/**
 * A setting that allows you to select the theme of the GUI.
 */
function ThemeSetting() {
  const theme = useSettings((s) => s.theme);
  const dispatch = useAppDispatch();

  return (
    <TableRow>
      <TableCell>
        <SettingLabel label="Theme" tooltip="This is the theme of the GUI." />
      </TableCell>
      <TableCell>
        <Select
          value={theme}
          onChange={(e) => dispatch(setTheme(e.target.value as Theme))}
          variant="outlined"
          fullWidth
        >
          {/** Create an option for each theme variant */}
          {Object.values(Theme).map((themeValue) => (
            <MenuItem key={themeValue} value={themeValue}>
              {themeValue.charAt(0).toUpperCase() + themeValue.slice(1)}
            </MenuItem>
          ))}
        </Select>
      </TableCell>
    </TableRow>
  );
}

/**
 * A modal containing a NodeTable for a given network and blockchain.
 * It allows you to add, remove, and move nodes up the list.
 */
function NodeTableModal({
  open,
  onClose,
  network,
  isValid,
  placeholder,
  blockchain,
}: {
  network: Network;
  blockchain: Blockchain;
  isValid: (url: string) => boolean;
  placeholder: string;
  open: boolean;
  onClose: () => void;
}) {
  return (
    <Dialog open={open} onClose={onClose}>
      <DialogTitle>Available Nodes</DialogTitle>
      <DialogContent>
        <Typography variant="subtitle2">
          When the daemon is started, it will attempt to connect to the first
          available {blockchain} node in this list. If you leave this field
          empty or all nodes are unavailable, it will choose from a list of
          known nodes at random.
        </Typography>
        <NodeTable
          network={network}
          blockchain={blockchain}
          isValid={isValid}
          placeholder={placeholder}
        />
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose} size="large">
          Close
        </Button>
      </DialogActions>
    </Dialog>
  );
}

// Create a circle SVG with a given color and radius
function Circle({ color, radius = 6 }: { color: string; radius?: number }) {
  return (
    <span>
      <svg
        width={radius * 2}
        height={radius * 2}
        viewBox={`0 0 ${radius * 2} ${radius * 2}`}
      >
        <circle cx={radius} cy={radius} r={radius} fill={color} />
      </svg>
    </span>
  );
}

/**
 * A table that displays the available nodes for a given network and blockchain.
 * It allows you to add, remove, and move nodes up the list.
 * It fetches the nodes from the store (nodesSlice) and the statuses of all nodes every 15 seconds.
 */
function NodeTable({
  network,
  blockchain,
  isValid,
  placeholder,
}: {
  network: Network;
  blockchain: Blockchain;
  isValid: (url: string) => boolean;
  placeholder: string;
}) {
  const availableNodes = useSettings((s) => s.nodes[network][blockchain]);
  const currentNode = availableNodes[0];
  const nodeStatuses = useNodes((s) => s.nodes);
  const [newNode, setNewNode] = useState("");
  const dispatch = useAppDispatch();

  const onAddNewNode = () => {
    dispatch(addNode({ network, type: blockchain, node: newNode }));
    setNewNode("");
  };

  const onRemoveNode = (node: string) =>
    dispatch(removeNode({ network, type: blockchain, node }));

  const onMoveUpNode = (node: string) =>
    dispatch(moveUpNode({ network, type: blockchain, node }));

  const moveUpButton = (node: string) => {
    if (currentNode === node) return <></>;

    return (
      <Tooltip title={"Move this node to the top of the list"}>
        <IconButton onClick={() => onMoveUpNode(node)} size="large">
          <ArrowUpward />
        </IconButton>
      </Tooltip>
    );
  };

  return (
    <TableContainer
      component={Paper}
      style={{ marginTop: "1rem" }}
      elevation={0}
    >
      <Table size="small">
        {/* Table header row */}
        <TableHead>
          <TableRow>
            <TableCell align="center">Node URL</TableCell>
            <TableCell align="center">Status</TableCell>
            <TableCell align="center">Actions</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>
          {/* Table body rows: one for each node */}
          {availableNodes.map((node, index) => (
            <TableRow key={index}>
              {/* Node URL */}
              <TableCell>
                <Typography variant="overline">{node}</Typography>
              </TableCell>
              {/* Node status icon */}
              <TableCell align="center">
                <Circle
                  color={nodeStatuses[blockchain][node] ? "green" : "red"}
                />
              </TableCell>
              {/* Remove and move buttons */}
              <TableCell>
                <Box style={{ display: "flex" }}>
                  <Tooltip
                    title={"Remove this node from your list"}
                    children={
                      <IconButton
                        onClick={() => onRemoveNode(node)}
                        children={<Delete />}
                        size="large"
                      />
                    }
                  />
                  {moveUpButton(node)}
                </Box>
              </TableCell>
            </TableRow>
          ))}
          {/* Last row: add a new node */}
          <TableRow key={-1}>
            <TableCell>
              <ValidatedTextField
                label="Add a new node"
                value={newNode}
                onValidatedChange={(value) => setNewNode(value ?? "")}
                placeholder={placeholder}
                fullWidth
                isValid={isValid}
                variant="outlined"
                noErrorWhenEmpty
              />
            </TableCell>
            <TableCell></TableCell>
            <TableCell>
              <Tooltip title={"Add this node to your list"}>
                <IconButton
                  onClick={onAddNewNode}
                  disabled={
                    availableNodes.includes(newNode) || newNode.length === 0
                  }
                  size="large"
                >
                  <Add />
                </IconButton>
              </Tooltip>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </TableContainer>
  );
}

function NetworkProxySetting() {
  const dispatch = useAppDispatch();
  const networkProxyMode = useSettings((s) => s.networkProxyMode);
  const torSocksAddress = useSettings((s) => s.torSocksAddress);
  const enableMoneroTor = useSettings((s) => s.enableMoneroTor);
  const allowDfxClearnet = useSettings((s) => s.allowDfxClearnet);

  const [addrInput, setAddrInput] = useState(torSocksAddress ?? "");
  const [probeStatus, setProbeStatus] = useState<boolean | null>(null);
  const [isRefreshing, setIsRefreshing] = useState(false);

  // Keep addrInput in sync with persisted state (e.g. after rehydration)
  useEffect(() => {
    setAddrInput(torSocksAddress ?? "");
  }, [torSocksAddress]);

  const handleAddrChange = (raw: string) => {
    // Strip any character that cannot appear in an IPv4 ip:port address.
    const val = raw.replace(/[^0-9.:]/g, "");
    setAddrInput(val);
    if (val === "") {
      dispatch(setTorSocksAddress(null));
      setProbeStatus(null);
    } else if (isValidSocksAddress(val)) {
      dispatch(setTorSocksAddress(val));
    }
  };

  const parsed = parseSocks5Address(addrInput);

  // Debounced auto-probe: wait until the user stops typing before firing a
  // SOCKS5 handshake, so each keystroke doesn't trigger its own TCP connect.
  // Only fires once the address is fully formed — `ipComplete`,
  // `awaitingPort`, and `partialIp` phases do not hit the network.
  useEffect(() => {
    if (networkProxyMode !== NetworkProxyMode.TorSocks || parsed.phase !== "complete") {
      setProbeStatus(null);
      return;
    }

    let cancelled = false;
    const timer = setTimeout(async () => {
      setIsRefreshing(true);
      const result = await probeSocks5(addrInput);
      if (!cancelled) {
        setProbeStatus(result);
        setIsRefreshing(false);
      }
    }, 400);

    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [addrInput, networkProxyMode, parsed.phase]);

  const handleRefresh = async () => {
    if (parsed.phase !== "complete") return;
    setIsRefreshing(true);
    setProbeStatus(await probeSocks5(addrInput));
    setIsRefreshing(false);
  };

  const addrIsError = parsed.phase === "invalid";
  const addrHelperText =
    parsed.phase === "invalid"
      ? parsed.error
      : parsed.phase === "partialIp" ||
          parsed.phase === "ipComplete" ||
          parsed.phase === "awaitingPort"
        ? parsed.hint
        : "";

  return (
    <>
      <TableRow>
        <TableCell>
          <SettingLabel
            label="Network proxy"
            tooltip="Configure how eigenwallet routes its network traffic. Changes take effect after restarting the app."
          />
        </TableCell>
        <TableCell>
          <ToggleButtonGroup
            color="primary"
            value={networkProxyMode}
            onChange={(_, newMode) => {
              if (
                newMode === NetworkProxyMode.InternalTor ||
                newMode === NetworkProxyMode.TorSocks ||
                newMode === NetworkProxyMode.None
              ) {
                dispatch(setNetworkProxyMode(newMode));
              }
            }}
            exclusive
            size="small"
          >
            <Tooltip title="Route all traffic through the built-in Tor instance. Recommended for most users.">
              <ToggleButton value={NetworkProxyMode.InternalTor}>
                Internal Tor (Recommended)
              </ToggleButton>
            </Tooltip>
            <Tooltip title="Route traffic through a system Tor SOCKS5 proxy on localhost. For power users running a local Tor daemon (e.g. on Tails).">
              <ToggleButton value={NetworkProxyMode.TorSocks}>
                Tor Socks (Advanced)
              </ToggleButton>
            </Tooltip>
            <Tooltip title="Connect directly to the network without any proxy. Onion peers are not available in this mode.">
              <ToggleButton value={NetworkProxyMode.None}>
                None
              </ToggleButton>
            </Tooltip>
          </ToggleButtonGroup>
        </TableCell>
      </TableRow>

      {networkProxyMode === NetworkProxyMode.InternalTor && (
        <TableRow>
          <TableCell>
            <SettingLabel
              label="Route Monero traffic through Tor"
              tooltip="When enabled, Monero wallet traffic will be routed through Tor for additional privacy. Requires the built-in Tor to be active."
            />
          </TableCell>
          <TableCell>
            <Switch
              checked={enableMoneroTor}
              onChange={(e) => dispatch(setEnableMoneroTor(e.target.checked))}
              color="primary"
            />
          </TableCell>
        </TableRow>
      )}

      {networkProxyMode === NetworkProxyMode.TorSocks && (
        <TableRow>
          <TableCell>
            <SettingLabel
              label="Tor Socks Address"
              tooltip="IPv4 address and port of the Tor SOCKS5 proxy (e.g. 127.0.0.1:9050 for a local Tor daemon, 10.152.152.10:9050 for Whonix). Only IPv4 addresses are accepted. Changes take effect after restarting the app."
            />
          </TableCell>
          <TableCell>
            <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
              <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
                <TextField
                  value={addrInput}
                  onChange={(e) => handleAddrChange(e.target.value)}
                  placeholder="127.0.0.1:9050"
                  error={addrIsError}
                  helperText={addrHelperText}
                  variant="outlined"
                  size="small"
                  fullWidth
                />
                <Tooltip
                  title={
                    parsed.phase === "empty"
                      ? "Enter an address to check proxy status"
                      : parsed.phase !== "complete"
                        ? "Finish typing the address to probe"
                        : probeStatus === null
                          ? "Checking proxy..."
                          : probeStatus
                            ? "Proxy is reachable"
                            : "Proxy is unreachable"
                  }
                >
                  <Box sx={{ display: "flex", alignItems: "center" }}>
                    <Circle
                      color={
                        parsed.phase !== "complete" || probeStatus === null
                          ? "gray"
                          : probeStatus
                            ? "green"
                            : "red"
                      }
                    />
                  </Box>
                </Tooltip>
                <Tooltip title="Check proxy availability">
                  <IconButton
                    onClick={handleRefresh}
                    disabled={isRefreshing || parsed.phase !== "complete"}
                    size="small"
                  >
                    {isRefreshing ? <HourglassEmpty /> : <Refresh />}
                  </IconButton>
                </Tooltip>
              </Box>
              {parsed.phase === "complete" &&
                !isSocksAddressLocalhost(addrInput) && (
                  <Alert severity="warning" variant="outlined">
                    SOCKS5 traffic between this app and the proxy is
                    unencrypted. Make sure the network path to{" "}
                    <Typography component="span" fontFamily="monospace" fontSize="inherit">
                      {addrInput}
                    </Typography>{" "}
                    is trusted (e.g. an isolated VM network or a secured LAN).
                  </Alert>
                )}
            </Box>
          </TableCell>
        </TableRow>
      )}

      <TableRow>
        <TableCell>
          <SettingLabel
            label="Enable DFX (clearnet only)"
            tooltip="Controls the DFX fiat on-ramp integration. DFX is reached over clearnet regardless of the proxy mode. When disabled, the Buy Monero entry is hidden and DFX is never contacted."
          />
        </TableCell>
        <TableCell>
          <Switch
            checked={allowDfxClearnet}
            onChange={(e) =>
              dispatch(setAllowDfxClearnet(e.target.checked))
            }
            color="primary"
          />
        </TableCell>
      </TableRow>
    </>
  );
}

/**
 * A setting that allows you to manage rendezvous points for maker discovery
 */
function RendezvousPointsSetting() {
  const [tableVisible, setTableVisible] = useState(false);
  const rendezvousPoints = useSettings((s) => s.rendezvousPoints);
  const dispatch = useAppDispatch();
  const [newPoint, setNewPoint] = useState("");

  const onAddNewPoint = () => {
    dispatch(addRendezvousPoint(newPoint));
    setNewPoint("");
  };

  const onRemovePoint = (point: string) => {
    dispatch(removeRendezvousPoint(point));
  };

  return (
    <TableRow>
      <TableCell>
        <SettingLabel
          label="Rendezvous Points"
          tooltip="These are the points where makers can be discovered. Add custom rendezvous points here to expand your maker discovery options."
        />
      </TableCell>
      <TableCell>
        <IconButton onClick={() => setTableVisible(true)}>
          <Edit />
        </IconButton>
        {tableVisible && (
          <Dialog
            open={true}
            onClose={() => setTableVisible(false)}
            maxWidth="md"
            fullWidth
          >
            <DialogTitle>Rendezvous Points</DialogTitle>
            <DialogContent>
              <Typography variant="subtitle2">
                Add or remove rendezvous points where makers can be discovered.
                These points help you find trading partners in a decentralized
                way.
              </Typography>
              <TableContainer
                component={Paper}
                style={{ marginTop: "1rem" }}
                elevation={0}
              >
                <Table size="small">
                  <TableHead>
                    <TableRow>
                      <TableCell style={{ width: "85%" }}>
                        Rendezvous Point
                      </TableCell>
                      <TableCell style={{ width: "15%" }} align="right">
                        Actions
                      </TableCell>
                    </TableRow>
                  </TableHead>
                  <TableBody>
                    {rendezvousPoints.map((point, index) => (
                      <TableRow key={index}>
                        <TableCell style={{ wordBreak: "break-all" }}>
                          <Typography variant="overline">{point}</Typography>
                        </TableCell>
                        <TableCell align="right">
                          <Tooltip title="Remove this rendezvous point">
                            <IconButton onClick={() => onRemovePoint(point)}>
                              <Delete />
                            </IconButton>
                          </Tooltip>
                        </TableCell>
                      </TableRow>
                    ))}
                    <TableRow>
                      <TableCell>
                        <ValidatedTextField
                          label="Add new rendezvous point"
                          value={newPoint}
                          onValidatedChange={(value) =>
                            setNewPoint(value ?? "")
                          }
                          placeholder="/dns4/rendezvous.observer/tcp/8888/p2p/12D3KooWMjceGXrYuGuDMGrfmJxALnSDbK4km6s1i1sJEgDTgGQa"
                          fullWidth
                          isValid={isValidMultiAddressWithPeerId}
                          variant="outlined"
                          noErrorWhenEmpty
                        />
                      </TableCell>
                      <TableCell align="right">
                        <Tooltip title="Add this rendezvous point">
                          <IconButton
                            onClick={onAddNewPoint}
                            disabled={
                              !isValidMultiAddressWithPeerId(newPoint) ||
                              newPoint.length === 0
                            }
                          >
                            <Add />
                          </IconButton>
                        </Tooltip>
                      </TableCell>
                    </TableRow>
                  </TableBody>
                </Table>
              </TableContainer>
            </DialogContent>
            <DialogActions>
              <Button onClick={() => setTableVisible(false)} size="large">
                Close
              </Button>
            </DialogActions>
          </Dialog>
        )}
      </TableCell>
    </TableRow>
  );
}

/**
 * A setting that allows you to set a development donation tip amount
 */
function DonationTipSetting() {
  const donateToDevelopment = useSettings((s) => s.donateToDevelopment);
  const [dialogOpen, setDialogOpen] = useState(false);

  return (
    <TableRow>
      <TableCell>
        <SettingLabel
          label="Tip to the developers"
          tooltip="Donates a small percentage of your swaps to fund development efforts"
        />
      </TableCell>
      <TableCell>
        <Box
          sx={{
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
          }}
        >
          <Typography variant="body1" sx={{ fontWeight: "bold" }}>
            {formatDonationTipLabel(donateToDevelopment)}
          </Typography>
          <Button
            variant="outlined"
            size="small"
            onClick={() => setDialogOpen(true)}
          >
            Change
          </Button>
        </Box>
        <DonationTipDialog
          open={dialogOpen}
          onClose={() => setDialogOpen(false)}
        />
      </TableCell>
    </TableRow>
  );
}

function RedeemPolicySetting() {
  const moneroRedeemPolicy = useSettings(
    (settings) => settings.moneroRedeemPolicy,
  );
  const moneroRedeemAddress = useSettings(
    (settings) => settings.externalMoneroRedeemAddress,
  );
  const dispatch = useAppDispatch();

  return (
    <>
      <TableRow>
        <TableCell>
          <SettingLabel
            label="Redeem Policy"
            tooltip="Where do you want Monero to be sent to in case of a successful swap? Choose between using the internal Monero wallet, or an external Monero address."
          />
        </TableCell>
        <TableCell>
          <ToggleButtonGroup
            color="primary"
            value={moneroRedeemPolicy}
            onChange={(_, newPolicy) => {
              if (
                newPolicy == RedeemPolicy.Internal ||
                newPolicy == RedeemPolicy.External
              ) {
                dispatch(setMoneroRedeemPolicy(newPolicy));
              }
            }}
            exclusive
            size="small"
          >
            <Tooltip title="The Monero will be sent to the currently opened Monero wallet.">
              <ToggleButton value={RedeemPolicy.Internal}>
                Internal (Recommended)
              </ToggleButton>
            </Tooltip>
            <Tooltip title="The Monero will be sent to an external Monero address.">
              <ToggleButton value={RedeemPolicy.External}>
                External
              </ToggleButton>
            </Tooltip>
          </ToggleButtonGroup>
        </TableCell>
      </TableRow>
      <TableRow>
        <TableCell>External Monero redeem address</TableCell>
        <TableCell>
          <MoneroAddressTextField
            disabled={moneroRedeemPolicy !== RedeemPolicy.External}
            label="External Monero redeem address"
            address={moneroRedeemAddress}
            onAddressChange={(address) => {
              dispatch(setMoneroRedeemAddress(address));
            }}
            fullWidth
            variant="outlined"
            allowEmpty={moneroRedeemPolicy === RedeemPolicy.Internal}
          />
        </TableCell>
      </TableRow>
    </>
  );
}

function RefundPolicySetting() {
  const bitcoinRefundPolicy = useSettings(
    (settings) => settings.bitcoinRefundPolicy,
  );
  const bitcoinRefundAddress = useSettings(
    (settings) => settings.externalBitcoinRefundAddress,
  );
  const dispatch = useAppDispatch();

  return (
    <>
      <TableRow>
        <TableCell>
          <SettingLabel
            label="Refund Policy"
            tooltip="Where do you want Bitcoin to be sent to in case of a successful swap? Choose between using the internal Bitcoin wallet, or an external Bitcoin address."
          />
        </TableCell>
        <TableCell>
          <ToggleButtonGroup
            color="primary"
            value={bitcoinRefundPolicy}
            onChange={(_, newPolicy) => {
              if (
                newPolicy == RefundPolicy.Internal ||
                newPolicy == RefundPolicy.External
              ) {
                dispatch(setBitcoinRefundPolicy(newPolicy));
              }
            }}
            exclusive
            size="small"
          >
            <Tooltip title="The Bitcoin will be sent to the internal Bitcoin wallet.">
              <ToggleButton value={RefundPolicy.Internal}>
                Internal (Recommended)
              </ToggleButton>
            </Tooltip>
            <Tooltip title="The Bitcoin will be sent to an external Bitcoin address.">
              <ToggleButton value={RefundPolicy.External}>
                External
              </ToggleButton>
            </Tooltip>
          </ToggleButtonGroup>
        </TableCell>
      </TableRow>
      <TableRow>
        <TableCell>External Bitcoin refund address</TableCell>
        <TableCell>
          <BitcoinAddressTextField
            allowEmpty={bitcoinRefundPolicy === RefundPolicy.Internal}
            label="External Bitcoin refund address"
            address={bitcoinRefundAddress}
            onAddressChange={(address) => {
              dispatch(setBitcoinRefundAddress(address));
            }}
            fullWidth
            variant="outlined"
            disabled={bitcoinRefundPolicy !== RefundPolicy.External}
            helperText=""
          />
        </TableCell>
      </TableRow>
    </>
  );
}
