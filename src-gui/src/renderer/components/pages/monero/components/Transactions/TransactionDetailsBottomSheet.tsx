import React from 'react'
import {
    Box,
    Drawer,
    IconButton,
    Typography,
    Stack,
    Chip,
    useTheme,
    Skeleton,
} from '@mui/material'
import {
    Close as CloseIcon,
} from '@mui/icons-material'
import { TransactionDirection, TransactionInfo } from 'models/tauriModel'
import {
    FiatPiconeroAmount,
    PiconeroAmount,
} from 'renderer/components/other/Units'
import dayjs from 'dayjs'
import ActionableMonospaceTextBox from 'renderer/components/other/ActionableMonospaceTextBox'
import ConfirmationsBadge from 'renderer/components/pages/monero/components/Transactions/ConfirmationsBadge'

interface TransactionDetailsBottomSheetProps {
    open: boolean
    onClose: () => void
    transaction: TransactionInfo | null
}

export default function TransactionDetailsBottomSheet({
    open,
    onClose,
    transaction,
}: TransactionDetailsBottomSheetProps) {
    const theme = useTheme()

    const handleCopyToClipboard = (text: string) => {
        navigator.clipboard.writeText(text)
        // Could add a toast notification here
    }

    const truncateAddress = (address: string, length: number = 10) => {
        if (address.length <= length * 2) return address
        return `${address.slice(0, length)}...${address.slice(-length)}`
    }

    // If transaction is not loaded, show loading skeleton
    if (!transaction) {
        return (
            <Drawer
                anchor="bottom"
                open={open}
                onClose={onClose}
                PaperProps={{
                    sx: {
                        borderTopLeftRadius: 16,
                        borderTopRightRadius: 16,
                        maxHeight: '80vh',
                        backgroundColor: 'background.paper',
                    },
                }}
            >
                <Box sx={{ p: 3, pb: 4 }}>
                    {/* Header Skeleton */}
                    <Box
                        sx={{
                            display: 'flex',
                            alignItems: 'center',
                            gap: 2,
                            mb: 3,
                        }}
                    >
                        <Skeleton variant="circular" width={32} height={32} />
                        <Skeleton variant="text" width={150} height={28} />
                    </Box>

                    {/* Transaction Summary Skeleton */}
                    <Box
                        sx={{
                            textAlign: 'center',
                            mb: 4,
                            py: 3,
                        }}
                    >
                        {/* Transaction type skeleton */}
                        <Skeleton
                            variant="text"
                            width={200}
                            height={40}
                            sx={{ mx: 'auto', mb: 1 }}
                        />

                        {/* Date skeleton */}
                        <Skeleton
                            variant="text"
                            width={150}
                            height={24}
                            sx={{ mx: 'auto', mb: 3 }}
                        />

                        {/* Amount skeleton */}
                        <Skeleton
                            variant="text"
                            width={250}
                            height={60}
                            sx={{ mx: 'auto', mb: 1 }}
                        />

                        {/* Fiat equivalent skeleton */}
                        <Skeleton
                            variant="text"
                            width={100}
                            height={24}
                            sx={{ mx: 'auto' }}
                        />
                    </Box>

                    {/* Transaction Details Skeleton */}
                    <Stack spacing={3}>
                        {/* Transaction ID field skeleton */}
                        <Box>
                            <Skeleton
                                variant="text"
                                width={120}
                                height={16}
                                sx={{ mb: 1 }}
                            />
                            <Skeleton
                                variant="rectangular"
                                height={48}
                                sx={{ borderRadius: 1 }}
                            />
                        </Box>

                        {/* Fees field skeleton */}
                        <Box>
                            <Skeleton
                                variant="text"
                                width={40}
                                height={16}
                                sx={{ mb: 1 }}
                            />
                            <Skeleton
                                variant="rectangular"
                                width={120}
                                height={32}
                                sx={{ borderRadius: 20 }}
                            />
                        </Box>
                    </Stack>
                </Box>
            </Drawer>
        )
    }

    const isIncoming = transaction.direction === TransactionDirection.In
    const displayDate = dayjs(transaction.timestamp * 1000).format(
        'MMM Do YYYY, HH:mm'
    )
    const transactionType = isIncoming ? 'Received Monero' : 'Sent Monero'

    // Placeholder address - in real implementation this would come from transaction data
    const fromAddress = '4A1...Bz9' // Placeholder

    return (
        <Drawer
            anchor="bottom"
            open={open}
            onClose={onClose}
            PaperProps={{
                sx: {
                    borderTopLeftRadius: 16,
                    borderTopRightRadius: 16,
                    minHeight: '80vh',
                    backgroundColor: 'background.paper',
                },
            }}
        >
            <Box sx={{ p: 3, pb: 4 }}>
                {/* Header */}
                <Box
                    sx={{
                        position: "relative",
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'center',
                        gap: 2,
                        mb: 3,
                    }}
                >
                    <IconButton onClick={onClose} size="small" sx={{position: "absolute", left: 0}}>
                        <CloseIcon />
                    </IconButton>
                    <Typography variant="h6" sx={{ fontWeight: 600 }}>
                        Details
                    </Typography>
                </Box>

                {/* Transaction Summary Section */}
                <Box
                    sx={{
                        textAlign: 'center',
                        mb: 4,
                        py: 3,
                    }}
                >
                    {/* Transaction type */}
                    <Typography
                        variant="h5"
                        sx={{
                            fontWeight: 600,
                            mb: 1,
                            color: 'text.primary',
                        }}
                    >
                        {transactionType}
                    </Typography>

                    {/* Date */}
                    <Typography
                        variant="body2"
                        sx={{
                            color: 'text.secondary',
                            mb: 3,
                        }}
                    >
                        {displayDate}
                    </Typography>

                    {/* Amount in XMR */}
                    <Box
                        sx={{
                            display: 'flex',
                            alignItems: 'center',
                            justifyContent: 'center',
                            gap: 0.5,
                            mb: 1,
                        }}
                    >
                        <Typography
                            variant="h4"
                            sx={{
                                fontWeight: 'bold',
                                color: isIncoming
                                    ? 'success.main'
                                    : 'error.main',
                                opacity: !isIncoming ? 1 : 0,
                            }}
                        >
                            {!isIncoming ? '−' : ''}
                        </Typography>
                        <Typography
                            variant="h4"
                            sx={{
                                fontWeight: 'bold',
                                color: isIncoming
                                    ? 'success.main'
                                    : 'error.main',
                            }}
                        >
                            <PiconeroAmount
                                amount={transaction.amount}
                                labelStyles={{ fontSize: 24, ml: -0.5 }}
                                disableTooltip
                            />
                        </Typography>
                    </Box>

                    {/* Fiat equivalent */}
                    <Typography
                        variant="body1"
                        sx={{
                            color: 'text.secondary',
                        }}
                    >
                        <FiatPiconeroAmount amount={transaction.amount} />
                    </Typography>
                </Box>

                {/* Transaction Details Section */}
                <Stack spacing={3}>
                    {/* Transaction ID field */}
                    <Box>
                        <Typography
                            variant="caption"
                            sx={{
                                color: 'text.secondary',
                                textTransform: 'uppercase',
                                fontWeight: 600,
                                letterSpacing: 1,
                                mb: 1,
                                display: 'block',
                            }}
                        >
                            Transaction ID
                        </Typography>
                        <ActionableMonospaceTextBox
                            content={transaction.tx_hash}
                            enableQrCode={false}
                        />
                    </Box>

                    {/* Fees field */}
                    <Box sx={{display: "flex", flexDirection: "row", gap: 2}}>
                        <Box sx={{flex: 1}}>
                            <Typography
                                variant="caption"
                                sx={{
                                    color: 'text.secondary',
                                    textTransform: 'uppercase',
                                    fontWeight: 600,
                                    letterSpacing: 1,
                                    mb: 1,
                                    display: 'block',
                                }}
                            >
                                Fees
                            </Typography>
                            <Box
                                sx={{
                                    display: 'flex',
                                    alignItems: 'center',
                                    gap: 0.5,
                                    fontFamily: 'monospace',
                                    backgroundColor: theme.palette.grey[900],
                                    p: 1,
                                    borderRadius: 2,
                                    width: "max-content",
                                }}
                            >
                                <PiconeroAmount
                                    amount={transaction.fee}
                                    fixedPrecision={12}
                                    labelStyles={{ fontSize: 14 }}
                                />
                            </Box>
                        </Box>

                        {/* Confirmations field */}
                        <Box sx={{flex: 0.5}}>
                            <Typography
                                variant="caption"
                                sx={{
                                    color: 'text.secondary',
                                    textTransform: 'uppercase',
                                    fontWeight: 600,
                                    letterSpacing: 1,
                                    mb: 1,
                                    display: 'block',
                                }}
                            >
                                Confirmations
                            </Typography>
                            <Box sx={{p: 1, display: "flex", justifyContent: "flex-end"}}>
                              {transaction.confirmations < 10 ? (
                                <ConfirmationsBadge confirmations={transaction.confirmations} />
                              ) : (
                                <Typography
                                    variant="body2"
                                    sx={{
                                      color: 'text.secondary',
                                    }}
                                    >
                                    {transaction.confirmations}
                                </Typography>
                              )}
                              </Box>
                        </Box>
                    </Box>
                </Stack>
            </Box>
        </Drawer>
    )
}
