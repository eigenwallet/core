import React from 'react'
import {
    Box,
    Typography,
    Card,
    CardContent,
    useTheme,
    LinearProgress,
} from '@mui/material'
import {
    PiconeroAmount,
    FiatPiconeroAmount,
} from 'renderer/components/other/Units'
import MoneroIcon from 'renderer/components/icons/MoneroIcon'
import {
    GetMoneroBalanceResponse,
    GetMoneroSyncProgressResponse,
} from 'models/tauriModel'
import { relative } from 'path'
import ShimmerTypography from 'renderer/components/other/ShimmerTypography'

interface MoneroWalletOverviewProps {
    balance: GetMoneroBalanceResponse | null
    syncProgress?: GetMoneroSyncProgressResponse
}

/**
 * Mobile-optimized Monero wallet overview component
 * Displays balance information in a compact card format
 */
export default function MoneroWalletOverview({
    balance,
    syncProgress,
}: MoneroWalletOverviewProps) {
    const theme = useTheme()

    const isSyncing = syncProgress && syncProgress.progress_percentage < 100
    const blocksLeft = syncProgress?.target_block - syncProgress?.current_block

    const pendingBalance = balance
        ? parseFloat(balance.total_balance) -
          parseFloat(balance.unlocked_balance)
        : 0

    return (
        <Card
            sx={{
                background:
                    theme.palette.mode === 'dark'
                        ? 'rgba(255,255,255,0.08)'
                        : '#f5f5f5',
                borderRadius: 3,
            }}
        >
            <CardContent sx={{ p: 2, "&:last-child": { pb: 2 }, position: 'relative' }}>
                <Box
                    sx={{
                        display: 'flex',
                        flexDirection: 'row',
                        alignItems: 'flex-end',
                        justifyContent: 'space-between',
                    }}
                >
                    <Box
                        sx={{
                            display: 'flex',
                            flexDirection: 'column',
                            gap: 1.5,
                        }}
                    >
                        <Box
                            sx={{
                                display: 'flex',
                                alignItems: 'center',
                                gap: 1,
                            }}
                        >
                            <MoneroIcon
                                sx={{
                                    fontSize: 16,
                                    color:
                                        theme.palette.mode === 'dark'
                                            ? '#FF6600'
                                            : '#FF6600',
                                }}
                            />
                            <Typography
                                variant="subtitle2"
                                color="text.secondary"
                            >
                                Monero
                            </Typography>
                        </Box>
                        <Typography variant="caption" color="text.secondary">
                            {balance && (
                                <FiatPiconeroAmount
                                    amount={parseFloat(
                                        balance.unlocked_balance
                                    )}
                                />
                            )}
                        </Typography>
                    </Box>
                    <Box>
                        <Typography variant="h4" fontWeight={700}>
                            {balance ? (
                                <PiconeroAmount
                                    amount={parseFloat(
                                        balance.unlocked_balance
                                    )}
                                    fixedPrecision={4}
                                    disableTooltip
                                    labelStyles={{ fontSize: 24 }}
                                />
                            ) : (
                                '--'
                            )}
                        </Typography>
                    </Box>
                </Box>
                {pendingBalance > 0 && (
                    <Box
                        sx={{
                            display: 'flex',
                            flexDirection: 'row',
                            gap: 1,
                            justifyContent: 'flex-end',
                            width: '100%',
                        }}
                    >
                        <ShimmerTypography
                            variant="body2"
                            color="warning"
                        >
                            Pending
                        </ShimmerTypography>
                        <Typography variant="body2" color="text.secondary">
                            <PiconeroAmount amount={pendingBalance} />
                        </Typography>
                    </Box>
                )}
                {isSyncing && (
                    <>
                        <ShimmerTypography
                            variant="body2"
                            color="text.secondary"
                            sx={{ position: 'relative', bottom: -10 }}
                        >
                            Syncing – {blocksLeft > 1 ? blocksLeft.toLocaleString() + " blocks left" : "1 block left"}
                        </ShimmerTypography>
                        <LinearProgress
                            value={syncProgress.progress_percentage}
                            variant="determinate"
                            sx={{
                                width: '100%',
                                position: 'absolute',
                                bottom: 0,
                                left: 0,
                            }}
                        />
                    </>
                )}
            </CardContent>
        </Card>
    )
}
