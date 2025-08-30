import { Card } from "@mui/material";
import { GetPgpInfoArgs, GetPgpInfoResponse } from "models/tauriModel";
import { useEffect, useState } from "react";
import MonospaceTextBox from "renderer/components/other/MonospaceTextBox";
import { getPgpInfo } from "renderer/rpc";

export default function PgpPage() {
  const [pgpInfo, setPgpInfo] = useState<GetPgpInfoResponse | null>(null);

  useEffect(() => {
    getPgpInfo().then((i) => setPgpInfo(i));
  }, []);

  return (
    <Card style={{ padding: 10 }}>
      <MonospaceTextBox>{pgpInfo?.fingerprint}</MonospaceTextBox>
      <div style={{ height: "2rem" }} />
      <MonospaceTextBox>{pgpInfo?.public_key}</MonospaceTextBox>
    </Card>
  );
}
