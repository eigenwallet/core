import React from "react";
import {
    Box,
    Typography,
    IconButton,
    Stack,
    Card,
    CardContent,
    Button,
    useTheme,
} from "@mui/material";
import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import SettingsIcon from "@mui/icons-material/Settings";
import ArrowDownwardIcon from "@mui/icons-material/ArrowDownward";
import ArrowUpwardIcon from "@mui/icons-material/ArrowUpward";
import SwapHorizIcon from "@mui/icons-material/SwapHoriz";
import MoreHorizIcon from "@mui/icons-material/MoreHoriz";
import HelpOutlineIcon from "@mui/icons-material/HelpOutline";
import AppsIcon from "@mui/icons-material/Apps";

/**
 * Mobile HomePage – placeholder implementation based on the provided mock-up.
 * All values are static for now and should be wired up to real data later.
 */
export default function HomePage() {
    const theme = useTheme();

    return (
        <Box
            sx={{
                p: 2,
                display: "flex",
                flexDirection: "column",
                gap: 3,
            }}
        >
            {/* Header */}
            <Stack direction="row" alignItems="center" spacing={2}>
                {/* Gradient avatar placeholder */}
                <Box
                    sx={{
                        width: 56,
                        height: 56,
                        borderRadius: "50%",
                        background: "radial-gradient(circle at 30% 30%, #00FFC2 0%, #004F3B 100%)",
                        border: `2px solid ${theme.palette.background.paper}`,
                    }}
                />
                <Typography variant="h5" fontWeight={600} flexGrow={1}>
                    Wallet 1
                    <ExpandMoreIcon fontSize="small" sx={{ ml: 0.5, verticalAlign: "middle" }} />
                </Typography>
                <IconButton size="small" color="inherit">
                    <SettingsIcon />
                </IconButton>
            </Stack>

            {/* Balances */}
            <Stack spacing={1}>
                {/* Primary balance card */}
                <Card
                    sx={{
                        background: theme.palette.mode === "dark" ? "rgba(255,255,255,0.08)" : "#f5f5f5",
                        borderRadius: 3,
                    }}
                >
                    <CardContent
                        sx={{ display: "flex", alignItems: "center", p: 2, "&:last-child": { pb: 2 } }}
                    >
                        <Box flexGrow={1}>
                            <Typography variant="subtitle2" color="text.secondary">
                                Monero
                            </Typography>
                            <Typography variant="caption" color="text.secondary">
                                (23.74 EUR)
                            </Typography>
                        </Box>
                        <Typography variant="h4" fontWeight={700} sx={{ mr: 1 }}>
                            0.3493
                        </Typography>
                        <Typography variant="subtitle1" fontWeight={600}>
                            XMR
                        </Typography>
                    </CardContent>
                </Card>

                {/* Secondary balance card */}
                <Card
                    sx={{
                        background: theme.palette.mode === "dark" ? "rgba(255,255,255,0.06)" : "#fafafa",
                        borderRadius: 3,
                    }}
                >
                    <CardContent
                        sx={{ display: "flex", alignItems: "center", p: 2, "&:last-child": { pb: 2 } }}
                    >
                        <Box flexGrow={1}>
                            <Typography variant="subtitle2" color="text.secondary">
                                Bitcoin
                            </Typography>
                        </Box>
                        <Typography variant="subtitle1" fontWeight={600} sx={{ mr: 1 }}>
                            0.0034
                        </Typography>
                        <Typography variant="subtitle2" fontWeight={500}>
                            BTC
                        </Typography>
                    </CardContent>
                </Card>
            </Stack>

            {/* Quick actions */}
            <Stack direction="row" justifyContent="space-between" textAlign="center">
                <ActionButton icon={<ArrowDownwardIcon />} label="Receive" />
                <ActionButton icon={<ArrowUpwardIcon />} label="Send" />
                <ActionButton icon={<SwapHorizIcon />} label="Swap" />
                <ActionButton icon={<MoreHorizIcon />} label="more" />
            </Stack>

            {/* Get Started */}
            <Box>
                <Typography variant="h6" gutterBottom>
                    Get Started
                </Typography>
                <Stack direction="row" spacing={2} sx={{ overflowX: "auto", pb: 1 }}>
                    <GetStartedCard
                        gradient="linear-gradient(135deg, #5b5bff 0%, #b85bff 100%)"
                        title="Begin Swaping"
                        subtitle="Swap BTC → XMR"
                    />
                    <GetStartedCard
                        gradient="linear-gradient(135deg, #ff8080 0%, #ff4d6d 100%)"
                        title="Introduction"
                        subtitle="What is eigenwalle"
                    />
                </Stack>
            </Box>

            {/* Transactions */}
            <Box flexGrow={1}>
                <Typography variant="h6" gutterBottom>
                    Transactions
                </Typography>
                <Stack
                    direction="row"
                    spacing={1}
                    alignItems="center"
                    color="text.secondary"
                    sx={{ opacity: 0.6 }}
                >
                    <AppsIcon />
                    <Typography variant="body2">Your transactions will show up here</Typography>
                </Stack>
            </Box>

            {/* Floating help button */}
            <IconButton
                sx={{
                    position: "fixed",
                    bottom: 24,
                    right: 24,
                    width: 48,
                    height: 48,
                    borderRadius: "50%",
                    backgroundColor: theme.palette.mode === "dark" ? "rgba(255,255,255,0.08)" : theme.palette.grey[200],
                    backdropFilter: "blur(10px)",
                    zIndex: theme.zIndex.tooltip,
                }}
            >
                <HelpOutlineIcon />
            </IconButton>
        </Box>
    );
}

// Reusable action button component
function ActionButton({ icon, label }: { icon: React.ReactNode; label: string }) {
    return (
        <Button
            variant="text"
            sx={{
                minWidth: 64,
                flexDirection: "column",
                color: "inherit",
                textTransform: "none",
                p: 0,
            }}
        >
            {icon}
            <Typography variant="caption" mt={0.5}>
                {label}
            </Typography>
        </Button>
    );
}

// Reusable Get Started card
function GetStartedCard({
    gradient,
    title,
    subtitle,
}: {
    gradient: string;
    title: string;
    subtitle: string;
}) {
    return (
        <Card
            sx={{
                minWidth: 220,
                borderRadius: 3,
                background: gradient,
                color: "#fff",
                flexShrink: 0,
            }}
        >
            <CardContent sx={{ p: 2, "&:last-child": { pb: 2 } }}>
                <Typography variant="subtitle1" fontWeight={600} gutterBottom>
                    {title}
                </Typography>
                <Typography variant="caption" sx={{ opacity: 0.9 }}>
                    {subtitle}
                </Typography>
            </CardContent>
        </Card>
    );
}
