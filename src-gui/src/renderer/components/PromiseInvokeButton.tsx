import {
  Button,
  ButtonProps,
  Chip,
  ChipProps,
  IconButton,
  IconButtonProps,
  Tooltip,
} from "@mui/material";
import CircularProgress from "@mui/material/CircularProgress";
import { ContextStatus } from "models/tauriModel";
import { isContextFullyInitialized } from "models/tauriModelExt";
import { useSnackbar } from "notistack";
import { ReactNode, useState } from "react";
import { useAppSelector, useIsContextAvailable } from "store/hooks";

interface PromiseInvokeButtonProps<T> {
  onSuccess?: (data: T) => void | null;
  onInvoke: () => Promise<T>;
  onPendingChange?: (isPending: boolean) => void | null;
  isLoadingOverride?: boolean;
  isIconButton?: boolean;
  isChipButton?: boolean;
  loadIcon?: ReactNode;
  disabled?: boolean;
  displayErrorSnackbar?: boolean;
  tooltipTitle?: string | null;
  // true means that the entire context must be available
  // false means that the context doesn't have to be available at all
  // a custom function means that the context must satisfy the function
  contextRequirement?: ((status: ContextStatus) => boolean) | false | true;
}

export default function PromiseInvokeButton<T>({
  disabled = false,
  onSuccess = null,
  onInvoke,
  children,
  startIcon,
  endIcon,
  loadIcon = null,
  isLoadingOverride = false,
  isIconButton = false,
  isChipButton = false,
  displayErrorSnackbar = false,
  onPendingChange = null,
  contextRequirement = true,
  tooltipTitle = null,
  ...rest
}: PromiseInvokeButtonProps<T> & ButtonProps) {
  const { enqueueSnackbar } = useSnackbar();
  const [isPending, setIsPending] = useState(false);

  const isLoading = isPending || isLoadingOverride;

  async function handleClick() {
    if (!isPending) {
      try {
        onPendingChange?.(true);
        setIsPending(true);
        const result = await onInvoke();
        onSuccess?.(result);
      } catch (err: unknown) {
        console.error(err);

        if (displayErrorSnackbar) {
          enqueueSnackbar(err as string, {
            autoHideDuration: 60 * 1000,
            variant: "error",
          });
        }
      } finally {
        setIsPending(false);
        onPendingChange?.(false);
      }
    }
  }

  const requiresContextButNotAvailable = useAppSelector((state) => {
    const status = state.rpc.status;

    if (contextRequirement === false) {
      return false;
    }

    if (contextRequirement === true || contextRequirement == null) {
      return !isContextFullyInitialized(status);
    }

    if (status == null || status.type === "error") {
      return true;
    }

    return !contextRequirement(status.status);
  });
  const isDisabled = disabled || isLoading || requiresContextButNotAvailable;

  const actualTooltipTitle =
    (requiresContextButNotAvailable
      ? "Wait for the application to load all required components"
      : tooltipTitle) ?? "";

  const resolvedLoadingIcon = loadIcon || (
    <CircularProgress size={isChipButton ? 18 : 24} color="inherit" />
  );

  if (isChipButton) {
    return (
      <Tooltip title={actualTooltipTitle}>
        <span>
          <Chip
            {...(rest as unknown as ChipProps)}
            onClick={handleClick}
            disabled={isDisabled}
            clickable={!isDisabled}
            variant="button"
            icon={
              <>{isLoading ? resolvedLoadingIcon : (endIcon ?? startIcon)}</>
            }
            label={children}
          />
        </span>
      </Tooltip>
    );
  }

  if (isIconButton) {
    return (
      <Tooltip title={actualTooltipTitle}>
        <span>
          <IconButton
            onClick={handleClick}
            disabled={isDisabled}
            {...(rest as IconButtonProps)}
            size="large"
            sx={{
              padding: "0.25rem",
            }}
          >
            {isLoading
              ? resolvedLoadingIcon
              : (children ?? endIcon ?? startIcon)}
          </IconButton>
        </span>
      </Tooltip>
    );
  }

  return (
    <Tooltip title={actualTooltipTitle}>
      <span>
        <Button
          onClick={handleClick}
          disabled={isDisabled}
          startIcon={startIcon}
          endIcon={isLoading ? resolvedLoadingIcon : endIcon}
          {...rest}
        >
          {children}
        </Button>
      </span>
    </Tooltip>
  );
}
