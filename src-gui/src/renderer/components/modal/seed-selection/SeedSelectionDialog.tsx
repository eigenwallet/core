import {
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  FormControl,
  FormControlLabel,
  Radio,
  RadioGroup,
  TextField,
  Typography,
  Button,
  Box,
  List,
  ListItem,
  ListItemButton,
  ListItemText,
  Divider,
  Card,
  CardContent,
} from "@mui/material";
import NewPasswordInput from "renderer/components/other/NewPasswordInput";
import { useState, useEffect } from "react";
import { usePendingSeedSelectionApproval } from "store/hooks";
import { resolveApproval, checkSeed } from "renderer/rpc";
import { SeedChoice } from "models/tauriModel";
import PromiseInvokeButton from "renderer/components/PromiseInvokeButton";
import { open } from "@tauri-apps/plugin-dialog";
import AddIcon from "@mui/icons-material/Add";
import RefreshIcon from "@mui/icons-material/Refresh";
import FolderOpenIcon from "@mui/icons-material/FolderOpen";
import SearchIcon from "@mui/icons-material/Search";

/**
 * Parses a block height input string and returns a number if valid, 0 for empty string, or false if invalid.
 *
 * @param blockheightInput - The input string representing a block height.
 * @returns A non-negative integer if the input is a valid positive number,
 *          0 if the input is an empty string,
 *          false if the input is invalid (non-numeric, negative, or NaN).
 *
 * @example
 * parseBlockHeightInput(""); // 0
 * parseBlockHeightInput("123"); // 123
 * parseBlockHeightInput("abc"); // false
 * parseBlockHeightInput("-1"); // false
 * parseBlockHeightInput("0"); // 0
 */
function parseBlockHeightInput(blockheightInput: string): number | false {
  if (blockheightInput.length === 0) {
    return 0;
  }

  if (!/^\d+$/.test(blockheightInput)) {
    return false;
  }

  const blockheightNum = parseInt(blockheightInput, 10);

  if (
    blockheightInput === "0" ||
    (blockheightNum && !Number.isNaN(blockheightNum) && blockheightNum >= 0)
  ) {
    return blockheightNum;
  }

  return false;
}

export default function SeedSelectionDialog() {
  const pendingApprovals = usePendingSeedSelectionApproval();
  const [selectedOption, setSelectedOption] = useState<
    SeedChoice["type"] | undefined
  >("RandomSeed");
  const [customSeed, setCustomSeed] = useState<string>("");
  const [blockheightInput, setBlockheightInput] = useState<string>("");
  const [isSeedValid, setIsSeedValid] = useState<boolean>(false);
  const [password, setPassword] = useState<string>("");
  const [isPasswordValid, setIsPasswordValid] = useState<boolean>(true);
  const [walletPath, setWalletPath] = useState<string>("");

  const approval = pendingApprovals[0];

  // Extract recent wallets from the approval request content
  const recentWallets =
    approval?.request?.type === "SeedSelection"
      ? approval.request.content.recent_wallets
      : [];

  useEffect(() => {
    if (selectedOption === "FromSeed" && customSeed.trim()) {
      checkSeed(customSeed.trim())
        .then((valid) => {
          setIsSeedValid(valid);
        })
        .catch(() => {
          setIsSeedValid(false);
        });
    } else {
      setIsSeedValid(false);
    }
  }, [customSeed, selectedOption]);

  // Auto-select the first recent wallet if available
  useEffect(() => {
    if (recentWallets.length > 0) {
      setSelectedOption("FromWalletPath");
      setWalletPath(recentWallets[0]);
    }
  }, [recentWallets.length]);

  const selectWalletFile = async () => {
    const selected = await open({
      multiple: false,
      directory: false,
    });

    if (selected) {
      setWalletPath(selected);
    }
  };

  const hasBlockheightInput = blockheightInput.length > 0;
  const isBlockheightValid = parseBlockHeightInput(blockheightInput) !== false;
  const isBlockheightInvalid =
    hasBlockheightInput && isBlockheightValid === false;

  const Legacy = async () => {
    if (!approval)
      throw new Error("No approval request found for seed selection");

    await resolveApproval<SeedChoice>(approval.request_id, {
      type: "Legacy",
    });
  };

  const accept = async () => {
    if (!approval)
      throw new Error("No approval request found for seed selection");

    let seedChoice: SeedChoice;

    switch (selectedOption) {
      case "RandomSeed":
        seedChoice = { type: "RandomSeed", content: { password } };
        break;

      case "FromSeed": {
        const parsedBlockHeight = parseBlockHeightInput(blockheightInput);

        if (parsedBlockHeight === false) {
          throw new Error("Invalid blockheight");
        }

        seedChoice = {
          type: "FromSeed",
          content: {
            seed: customSeed,
            password,
            restore_height: parsedBlockHeight,
          },
        };
        break;
      }

      default:
        seedChoice = {
          type: "FromWalletPath",
          content: { wallet_path: walletPath },
        };
        break;
    }

    await resolveApproval<SeedChoice>(approval.request_id, seedChoice);
  };

  if (!approval) {
    return null;
  }

  // Disable the button if the user is restoring from a seed and the seed is invalid
  // or if selecting wallet path and no path is selected,
  // or if blockheight is provided but invalid,
  // or if setting a password and they don't match
  const isDisabled =
    selectedOption === "FromSeed"
      ? customSeed.trim().length === 0 ||
        !isSeedValid ||
        isBlockheightInvalid ||
        !isPasswordValid
      : selectedOption === "FromWalletPath"
        ? !walletPath
        : selectedOption === "RandomSeed"
          ? !isPasswordValid
          : false;

  return (
    <Dialog
      open={true}
      maxWidth="sm"
      fullWidth
      sx={{ "& .MuiDialog-paper": { minHeight: "min(32rem, 80vh)" } }}
      BackdropProps={{
        sx: {
          backdropFilter: "blur(8px)",
          backgroundColor: "rgba(0, 0, 0, 0.5)",
        },
      }}
    >
      <DialogContent sx={{ display: "flex", flexDirection: "column", gap: 3 }}>
        <Box sx={{ display: "flex", flexDirection: "row", gap: 2 }}>
          {/* Open existing wallet option */}
          <Card
            sx={{
              cursor: "pointer",
              border: selectedOption === "FromWalletPath" ? 2 : 1,
              borderColor:
                selectedOption === "FromWalletPath"
                  ? "primary.main"
                  : "divider",
              "&:hover": { borderColor: "primary.main" },
              flex: 1,
            }}
            onClick={() => setSelectedOption("FromWalletPath")}
          >
            <CardContent
              sx={{
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
                justifyContent: "center",
                gap: 1,
              }}
            >
              <FolderOpenIcon sx={{ fontSize: 32, color: "text.secondary" }} />
              <Typography
                variant="caption"
                color="text.secondary"
                sx={{ textAlign: "center" }}
              >
                Open wallet file
              </Typography>
            </CardContent>
          </Card>

          {/* Create new wallet option */}
          <Card
            sx={{
              cursor: "pointer",
              border: selectedOption === "RandomSeed" ? 2 : 1,
              borderColor:
                selectedOption === "RandomSeed" ? "primary.main" : "divider",
              "&:hover": { borderColor: "primary.main" },
              flex: 1,
            }}
            onClick={() => setSelectedOption("RandomSeed")}
          >
            <CardContent
              sx={{
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
                justifyContent: "center",
                gap: 1,
              }}
            >
              <AddIcon sx={{ fontSize: 32, color: "text.secondary" }} />
              <Typography
                variant="caption"
                color="text.secondary"
                sx={{ textAlign: "center" }}
              >
                Create new wallet
              </Typography>
            </CardContent>
          </Card>

          {/* Restore from seed option */}
          <Card
            sx={{
              cursor: "pointer",
              border: selectedOption === "FromSeed" ? 2 : 1,
              borderColor:
                selectedOption === "FromSeed" ? "primary.main" : "divider",
              "&:hover": { borderColor: "primary.main" },
              flex: 1,
            }}
            onClick={() => setSelectedOption("FromSeed")}
          >
            <CardContent
              sx={{
                display: "flex",
                flexDirection: "column",
                alignItems: "center",
                justifyContent: "center",
                gap: 1,
              }}
            >
              <RefreshIcon sx={{ fontSize: 32, color: "text.secondary" }} />
              <Typography
                variant="caption"
                color="text.secondary"
                sx={{ textAlign: "center" }}
              >
                Restore from seed
              </Typography>
            </CardContent>
          </Card>
        </Box>

        {selectedOption === "RandomSeed" && (
          <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
            <NewPasswordInput
              password={password}
              setPassword={setPassword}
              isPasswordValid={isPasswordValid}
              setIsPasswordValid={setIsPasswordValid}
            />

            <Typography
              variant="body2"
              color="text.secondary"
              sx={{ textAlign: "center" }}
            >
              A new wallet with a random seed phrase will be generated.
            </Typography>
            <Typography
              variant="caption"
              color="text.secondary"
              sx={{ textAlign: "center" }}
            >
              You will have the option to back it up later.
            </Typography>
          </Box>
        )}

        {selectedOption === "FromSeed" && (
          <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
            <TextField
              fullWidth
              multiline
              autoFocus
              rows={3}
              label="Enter your seed phrase"
              value={customSeed}
              onChange={(e) => setCustomSeed(e.target.value)}
              placeholder="Enter your Monero 25 words seed phrase..."
              error={!isSeedValid && customSeed.length > 0}
              helperText={
                isSeedValid
                  ? "Seed is valid"
                  : customSeed.length > 0
                    ? "Seed is invalid"
                    : ""
              }
            />

            <TextField
              type="text"
              inputProps={{
                inputmode: "numeric",
                pattern: "[0-9]*",
              }}
              label="Restore blockheight (optional)"
              value={blockheightInput}
              onChange={(e) => setBlockheightInput(e.target.value)}
              placeholder="Enter restore blockheight, leave empty to scan from the blockchain start"
              error={isBlockheightInvalid}
              helperText={
                isBlockheightInvalid
                  ? "Please enter a valid blockheight"
                  : hasBlockheightInput && isBlockheightValid
                    ? "Valid blockheight"
                    : ""
              }
            />

            <NewPasswordInput
              password={password}
              setPassword={setPassword}
              isPasswordValid={isPasswordValid}
              setIsPasswordValid={setIsPasswordValid}
              autoFocus={false}
            />
          </Box>
        )}

        {selectedOption === "FromWalletPath" && (
          <Box sx={{ gap: 2, display: "flex", flexDirection: "column" }}>
            <Box sx={{ display: "flex", gap: 1, alignItems: "center" }}>
              <TextField
                fullWidth
                label="Wallet file path"
                value={walletPath || ""}
                placeholder="Select a wallet file..."
                InputProps={{
                  readOnly: true,
                }}
              />
              <Button
                variant="outlined"
                onClick={selectWalletFile}
                sx={{ minWidth: "120px", height: "56px" }}
                startIcon={<SearchIcon />}
              >
                Browse
              </Button>
            </Box>
            {recentWallets.length > 0 && (
              <Box>
                <Box
                  sx={{
                    border: 1,
                    borderColor: "divider",
                    borderRadius: 1,
                    maxHeight: 200,
                    overflowY: "scroll",
                    "&::-webkit-scrollbar": {
                      display: "block !important",
                      width: "8px !important",
                    },
                    "&::-webkit-scrollbar-track": {
                      display: "block !important",
                      background: "rgba(255,255,255,.1) !important",
                      borderRadius: "4px",
                    },
                    "&::-webkit-scrollbar-thumb": {
                      display: "block !important",
                      background: "rgba(255,255,255,.6) !important",
                      borderRadius: "4px",
                      minHeight: "20px !important",
                    },
                    "&::-webkit-scrollbar-thumb:hover": {
                      background: "rgba(255,255,255,.8) !important",
                    },
                    "&::-webkit-scrollbar-corner": {
                      background: "transparent !important",
                    },
                    scrollbarWidth: "thin",
                    scrollbarColor: "rgba(255,255,255,.6) rgba(255,255,255,.1)",
                  }}
                >
                  <List disablePadding>
                    {recentWallets.map((path, index) => (
                      <Box key={index}>
                        <ListItem disablePadding>
                          <ListItemButton
                            selected={walletPath === path}
                            onClick={() => setWalletPath(path)}
                          >
                            <ListItemText
                              primary={path.split("/").pop() || path}
                              secondary={path}
                              primaryTypographyProps={{
                                fontWeight: walletPath === path ? 600 : 400,
                                fontSize: "0.9rem",
                              }}
                              secondaryTypographyProps={{
                                fontSize: "0.75rem",
                                sx: {
                                  overflow: "hidden",
                                  textOverflow: "ellipsis",
                                  whiteSpace: "nowrap",
                                },
                              }}
                            />
                          </ListItemButton>
                        </ListItem>
                        {index < recentWallets.length - 1 && <Divider />}
                      </Box>
                    ))}
                  </List>
                </Box>
              </Box>
            )}
          </Box>
        )}
      </DialogContent>
      <DialogActions sx={{ justifyContent: "space-between" }}>
        <PromiseInvokeButton
          variant="text"
          onInvoke={Legacy}
          contextRequirement={false}
          color="inherit"
        >
          No wallet (Legacy)
        </PromiseInvokeButton>
        <PromiseInvokeButton
          onInvoke={accept}
          variant="contained"
          disabled={isDisabled}
          contextRequirement={false}
        >
          Continue
        </PromiseInvokeButton>
      </DialogActions>
    </Dialog>
  );
}
