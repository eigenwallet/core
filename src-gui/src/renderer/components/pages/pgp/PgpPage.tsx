import { Card, Box, TextField, Button } from "@mui/material";
import { GetPgpInfoArgs, GetPgpInfoResponse } from "models/tauriModel";
import { useEffect, useState } from "react";
import ActionableMonospaceTextBox from "renderer/components/other/ActionableMonospaceTextBox";
import { decryptPgpMessage, getPgpInfo } from "renderer/rpc";

export default function PgpPage() {
  const [pgpInfo, setPgpInfo] = useState<GetPgpInfoResponse | null>(null);
  const [ciphertext, setCiphertext] = useState<string>("");
  const [message, setMessage] = useState<string>("");

  useEffect(() => {
    getPgpInfo().then((i) => setPgpInfo(i));
  }, []);

  return (
    <Box>
      <Card style={{ padding: "1rem" }}>
        <ActionableMonospaceTextBox
          enableQrCode={false}
          content={pgpInfo?.fingerprint}
        />
      </Card>

      <Card>
        <ActionableMonospaceTextBox
          enableQrCode={false}
          content={pgpInfo?.public_key}
        />
      </Card>

      <Card>
        <TextField
          placeholder="-----BEGIN PGP MESSAGE-----"
          onChange={(e) => {
            setCiphertext(e.target.value);
          }}
        />
        <Button
          onClick={async () => {
            setMessage("");
            const message = await decryptPgpMessage(ciphertext);
            setMessage(message);
          }}
          variant="contained"
        >
          Decrypt
        </Button>
      </Card>

      <Card>
        <TextField disabled={true} value={message} placeholder="message" />
      </Card>
    </Box>
  );
}
