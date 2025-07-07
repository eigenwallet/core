import { createTheme, ThemeOptions } from "@mui/material";
import { indigo } from "@mui/material/colors";

// Extend the theme to include custom chip variants
declare module "@mui/material/Chip" {
  interface ChipPropsVariantOverrides {
    button: true;
  }
}

export enum Theme {
  Light = "light",
  Dark = "dark",
}

const baseTheme: ThemeOptions = {
  typography: {
    overline: {
      textTransform: "none" as const,
      fontFamily: "monospace",
    },
  },
  breakpoints: {
    values: {
      xs: 0,
      sm: 600,
      md: 900,
      lg: 1000,
      xl: 1536,
    },
  },
  components: {
    MuiButton: {
      styleOverrides: {
        outlined: {
          color: "inherit",
          borderColor: "color-mix(in srgb, currentColor 30%, transparent)",
          "&:hover": {
            borderColor: "color-mix(in srgb, currentColor 30%, transparent)",
            backgroundColor: "color-mix(in srgb, #bdbdbd 10%, transparent)",
          },
        },
      },
    },
    MuiChip: {
      variants: [
        {
          props: { variant: "button" },
          style: ({ theme }) => ({
            padding: "12px 16px",
            cursor: "pointer",
          }),
          defaultProps: {
            clickable: true,
          },
        },
      ],
    },
    MuiDialog: {
      defaultProps: {
        slotProps: {
          paper: {
            variant: "outlined",
          },
        },
      },
    },
    MuiDialogContentText: {
      styleOverrides: {
        root: {
          marginBottom: "0.5rem",
        },
      },
    },
    MuiTextField: {
      styleOverrides: {
        root: {
          "& legend": {
            transition: "unset",
          },
        },
      },
    },
  },
};

const darkTheme = createTheme({
  ...baseTheme,
  palette: {
    mode: "dark",
    primary: {
      main: "#f4511e", // Monero orange
    },
    secondary: indigo,
  },
});

const lightTheme = createTheme({
  ...baseTheme,
  palette: {
    mode: "light",
    primary: {
      main: "#f4511e", // Monero orange
    },
    secondary: indigo,
  },
});

console.log("Creating themes:", {
  dark: darkTheme,
  light: lightTheme,
});

export const themes = {
  [Theme.Dark]: darkTheme,
  [Theme.Light]: lightTheme,
};
