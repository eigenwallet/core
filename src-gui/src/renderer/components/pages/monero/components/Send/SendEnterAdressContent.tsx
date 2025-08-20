import { Box, Button, Skeleton, Typography } from '@mui/material'
import { useEffect, useState } from 'react'
import MoneroAddressTextField from 'renderer/components/inputs/MoneroAddressTextField'
import { getMoneroAddresses } from 'renderer/rpc'

export default function SendEnterAddressContent({
    onContinue,
    address,
    onAddressChange,
    onAddressValidityChange,
}: {
    onContinue: () => void
    address: string
    onAddressChange: (address: string) => void
    onAddressValidityChange: (valid: boolean) => void
}) {
    const [isValidAddress, setIsValidAddress] = useState(false)
    const [previousAddresses, setPreviousAddresses] = useState<string[]>([])
    const [historyLoading, setHistoryLoading] = useState(false)

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
        fetchAddresses()
    }, [])

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
            <Box sx={{ display: 'flex', flexDirection: 'column', gap: 3, width: '100%' }}>
                <Typography variant="h6" align="center">
                    Select Recepient
                </Typography>

                <Box>
                    <Typography variant="body1" sx={{pb: 1}}>To</Typography>
                    <MoneroAddressTextField
                        address={address}
                        onAddressChange={onAddressChange}
                        onAddressValidityChange={handleValidityChange}
                        label="Monero Address"
                        disableHistory={true}
                        fullWidth
                    />
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
            <Button onClick={onContinue} disabled={!isValidAddress} variant='contained'>
                Continue
            </Button>
        </Box>
    )
}
