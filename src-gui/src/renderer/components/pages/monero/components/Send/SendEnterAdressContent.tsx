import { Box, Button, Card, Skeleton, Typography } from '@mui/material'
import { readText } from '@tauri-apps/plugin-clipboard-manager'
import { useEffect, useState } from 'react'
import MoneroAddressTextField from 'renderer/components/inputs/MoneroAddressTextField'
import { getMoneroAddresses } from 'renderer/rpc'
import { isTestnet } from 'store/config'
import { isXmrAddressValid } from 'utils/conversionUtils'
import ContentPasteIcon from '@mui/icons-material/ContentPaste'

export default function SendEnterAddressContent({
    open,
    onContinue,
    address,
    onAddressChange,
    onAddressValidityChange,
}: {
    open: boolean
    onContinue: () => void
    address: string
    onAddressChange: (address: string) => void
    onAddressValidityChange: (valid: boolean) => void
}) {
    const [isValidAddress, setIsValidAddress] = useState(false)
    const [previousAddresses, setPreviousAddresses] = useState<string[]>([])
    const [historyLoading, setHistoryLoading] = useState(false)
    const [clipboardAddress, setClipboardAddress] = useState<string | null>(
        null
    )

    const handleValidityChange = (valid: boolean) => {
        setIsValidAddress(valid)
        onAddressValidityChange(valid)
    }

    useEffect(() => {
        setHistoryLoading(true)
        const fetchAddresses = async () => {
            const response = await getMoneroAddresses()
            setPreviousAddresses(response.addresses)
            setHistoryLoading(false)
        }

        const getClipBoardAddress = async () => {
            const clipboardAddress = await readText()
            if (
                clipboardAddress &&
                isXmrAddressValid(clipboardAddress, isTestnet())
            ) {
                setClipboardAddress(clipboardAddress)
            }
        }

        fetchAddresses()
        getClipBoardAddress()
    }, [open])

    return (
        <Box
            sx={{
                width: '100%',
                flex: 1,
                display: 'flex',
                flexDirection: 'column',
                justifyContent: 'space-between',
                alignItems: 'center',
            }}
        >
            <Box
                sx={{
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 3,
                    width: '100%',
                }}
            >
                <Typography variant="h6" align="center">
                    Select Recepient
                </Typography>

                <Box>
                    <Typography variant="body1" sx={{ pb: 1 }}>
                        To
                    </Typography>
                    <MoneroAddressTextField
                        address={address}
                        onAddressChange={onAddressChange}
                        onAddressValidityChange={handleValidityChange}
                        label="Monero Address"
                        disableHistory={true}
                        fullWidth
                    />
                    {clipboardAddress && (
                        <Card
                            elevation={1}
                            sx={{ p: 2, bgcolor: 'grey.800', borderRadius: 2, mt: 1, cursor: 'pointer' }}
                            onClick={() => onAddressChange(clipboardAddress)}
                        >
                            <Box
                                sx={{
                                    display: 'flex',
                                    alignItems: 'center',
                                    gap: 1,
                                }}
                            >
                                <ContentPasteIcon />
                                <Typography>Paste from Clipboard</Typography>
                            </Box>

                            <Typography
                                sx={{
                                    fontFamily: 'monospace',
                                    width: '100%',
                                    display: 'block',
                                    pt: 1,
                                }}
                                variant="caption"
                                color="text.secondary"
                                noWrap
                            >
                                {clipboardAddress}
                            </Typography>
                        </Card>
                    )}
                </Box>

                <Box>
                    <Typography>History</Typography>
                    {historyLoading && (
                        <Skeleton variant="rounded" width="100%" height={40} />
                    )}
                    {!historyLoading && (
                        <Box
                            sx={{
                                display: 'flex',
                                flexDirection: 'column',
                                gap: 1,
                            }}
                        >
                            {previousAddresses.map((addr) => (
                                <Typography
                                    key={addr}
                                    onClick={() => onAddressChange(addr)}
                                    sx={{
                                        fontFamily: 'monospace',
                                        width: '100%',
                                        display: 'block',
                                        py: 1,
                                        cursor: 'pointer',
                                    }}
                                    color="text.secondary"
                                    noWrap
                                >
                                    {addr}
                                </Typography>
                            ))}
                        </Box>
                    )}
                </Box>
            </Box>
            <Button
                onClick={onContinue}
                disabled={!isValidAddress}
                variant="contained"
                sx={{
                    width: '100%',
                    p: 2,
                    fontSize: 16,
                    borderRadius: 3,
                }}
            >
                Continue
            </Button>
        </Box>
    )
}
