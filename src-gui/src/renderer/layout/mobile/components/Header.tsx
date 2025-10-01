import {
    IconButton,
    DialogTitle,
    DialogContent,
    Button,
    Dialog,
    Fade,
    Drawer,
    useTheme,
} from '@mui/material'
import { Stack } from '@mui/material'
import AvatarWithProgress from 'renderer/components/other/AvatarWithProgress'
import { useNavigate } from 'react-router-dom'
import EigenwalletIcon from 'renderer/components/icons/EigenwalletIcon'
import SettingsIcon from '@mui/icons-material/Settings'
import ShimmerTypography from 'renderer/components/other/ShimmerTypography'
import { useState } from 'react'
import { useDisplayWalletState } from 'utils/useDisplayWalletState'
import DaemonStatusAlert, {
    BackgroundProgressAlerts,
} from 'renderer/components/alert/DaemonStatusAlert'
import FundsLeftInWalletAlert from 'renderer/components/alert/FundsLeftInWalletAlert'
import UnfinishedSwapsAlert from 'renderer/components/alert/UnfinishedSwapsAlert'

export default function Header() {
    const navigate = useNavigate()
    const theme = useTheme()

    const { progress, stateLabel, isLoading, isError } = useDisplayWalletState()
    const [avatarDialogOpen, setAvatarDialogOpen] = useState(false)

    return (
        <>
            <Stack direction="row" alignItems="center" spacing={2}>
                {/* Avatar with instagram-like ring and progress */}
                <AvatarWithProgress
                    size={56}
                    progress={progress}
                    gradientSeed="to be replaced with wallet keyy"
                    onClick={() => setAvatarDialogOpen(true)}
                />
                <Stack spacing={-2} sx={{ transform: "translateY(-4px) translateX(-4px)", flexGrow: 1, lineHeight: 1 }}>
                <EigenwalletIcon
                    sx={{
                        fontSize: 64,
                        color:
                            theme.palette.mode === 'dark'
                                ? 'white'
                                : 'black',
                    }}
                />
                    {(
                        <ShimmerTypography
                            variant="caption"
                            sx={{ opacity: 0.85 }}
                            active={isLoading}
                        >
                            {stateLabel}
                        </ShimmerTypography>
                    )}
                </Stack>
                <IconButton
                    size="small"
                    color="inherit"
                    onClick={() =>
                        navigate('/settings', { viewTransition: true })
                    }
                >
                    <SettingsIcon />
                </IconButton>
            </Stack>
            <Drawer
                anchor="bottom"
                open={avatarDialogOpen}
                onClose={() => setAvatarDialogOpen(false)}
                sx={{
                    "& .MuiDrawer-paper": {
                        borderTopLeftRadius: 16,
                        borderTopRightRadius: 16,
                        pb: 4,
                    },
                }}
            >
                <DialogTitle>Wallet State</DialogTitle>
                <DialogContent>
                    <Stack spacing={2} sx={{ pt: 1 }}>
                        <DaemonStatusAlert />
                        <BackgroundProgressAlerts />
                        <UnfinishedSwapsAlert />
                    </Stack>
                </DialogContent>
            </Drawer>
        </>
    )
}
