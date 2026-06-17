import React, { ReactNode } from "react";
import { Box, Link, Typography } from "@mui/material";
import InfoBox from "./InfoBox";
import TruncatedText from "renderer/components/other/TruncatedText";

export type TransactionInfoBoxProps = {
  title: string;
  txId: string | null;
  explorerUrlCreator: ((txId: string) => string) | null;
  additionalContent: ReactNode;
  loading: boolean;
  icon: ReactNode;
  secondaryAction?: ReactNode;
};

export default function TransactionInfoBox({
  title,
  txId,
  additionalContent,
  icon,
  loading,
  explorerUrlCreator,
  secondaryAction,
}: TransactionInfoBoxProps) {
  return (
    <InfoBox
      title={title}
      mainContent={
        <Typography variant="h5">
          <TruncatedText truncateMiddle limit={40}>
            {txId ?? "Transaction ID not available"}
          </TruncatedText>
        </Typography>
      }
      loading={loading}
      additionalContent={
        <Box sx={{ display: "flex", flexDirection: "column", gap: 1 }}>
          <Typography variant="subtitle2">{additionalContent}</Typography>
          {((explorerUrlCreator != null && txId != null) || secondaryAction) && (
            <Box sx={{ display: "flex", alignItems: "center", gap: 1 }}>
              {explorerUrlCreator != null && txId != null && (
                <Typography variant="body1">
                  <Link href={explorerUrlCreator(txId)} target="_blank">
                    View on explorer
                  </Link>
                </Typography>
              )}
              {secondaryAction && (
                <Box sx={{ ml: "auto" }}>{secondaryAction}</Box>
              )}
            </Box>
          )}
        </Box>
      }
      icon={icon}
    />
  );
}
