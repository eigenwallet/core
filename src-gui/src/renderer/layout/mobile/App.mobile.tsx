import { HelpOutline } from '@mui/icons-material'
import { Box, IconButton, useTheme } from '@mui/material'
import {
    Route,
    MemoryRouter as Router,
    Routes,
    useNavigate,
} from 'react-router-dom'
import IntroductionModal from 'renderer/components/modal/introduction/IntroductionModal'
import PasswordEntryDialog from 'renderer/components/modal/password-entry/PasswordEntryDialog'
import SeedSelectionDialog from 'renderer/components/modal/seed-selection/SeedSelectionDialog'
import UpdaterDialog from 'renderer/components/modal/updater/UpdaterDialog'
import HomePage from './pages/HomePage'

import GlobalSnackbarProvider from 'renderer/components/snackbar/GlobalSnackbarProvider'
import SettingsPage from './pages/SettingsPage'
import FeedbackPage from './pages/FeedbackPage'
import TransactionsPage from './pages/HistoryPage'

export default function App() {
    return (
        <GlobalSnackbarProvider>
            <IntroductionModal />
            <SeedSelectionDialog />
            <PasswordEntryDialog />
            <Router>
                <InnerContent />
                <UpdaterDialog />
            </Router>
        </GlobalSnackbarProvider>
    )
}

function InnerContent() {
    const theme = useTheme()
    const navigate = useNavigate()
    return (
        <Box
            sx={{
                flex: 1,
            }}
        >
            <Routes>
                <Route path="/" element={<HomePage />} />
                <Route path="/settings" element={<SettingsPage />} />
                <Route path="/feedback" element={<FeedbackPage />} />
                <Route path="/transactions" element={<TransactionsPage />} />
            </Routes>
            {/* Floating help button */}
            <IconButton
                sx={{
                    position: 'fixed',
                    bottom: 24,
                    right: 24,
                    width: 48,
                    height: 48,
                    borderRadius: '50%',
                    backgroundColor:
                        theme.palette.mode === 'dark'
                            ? 'rgba(255,255,255,0.08)'
                            : theme.palette.grey[200],
                    backdropFilter: 'blur(10px)',
                    zIndex: theme.zIndex.fab,
                }}
                onClick={() => navigate('/feedback', { viewTransition: true })}
            >
                <HelpOutline />
            </IconButton>
        </Box>
    )
}
