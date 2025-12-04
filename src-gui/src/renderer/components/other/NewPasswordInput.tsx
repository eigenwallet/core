import { TextField, IconButton, InputAdornment } from "@mui/material";
import { useState, useEffect, Dispatch, SetStateAction } from "react";
import { Visibility, VisibilityOff } from "@mui/icons-material";

export default function NewPasswordInput({
  password,
  setPassword,
  isPasswordValid,
  setIsPasswordValid,
  autoFocus = true,
}: {
  password: string;
  setPassword: Dispatch<SetStateAction<string>>;
  isPasswordValid: boolean;
  setIsPasswordValid: Dispatch<SetStateAction<boolean>>;
  autoFocus?: boolean;
}) {
  const [passwordRepeat, setPasswordRepeat] = useState<string>("");
  const [showPassword, setShowPassword] = useState<boolean>(false);

  useEffect(() => {
    setIsPasswordValid(password === passwordRepeat);
  }, [password, passwordRepeat, setIsPasswordValid]);

  return (
    <>
      <TextField
        fullWidth
        margin="dense"
        type={showPassword ? "text" : "password"}
        label="Set password (leave blank to go passwordless)"
        value={password}
        onChange={(e) => setPassword(e.target.value)}
        error={!isPasswordValid}
        autoFocus={autoFocus}
      />

      <TextField
        fullWidth
        margin="none"
        type={showPassword ? "text" : "password"}
        label="Repeat password"
        value={passwordRepeat}
        onChange={(e) => setPasswordRepeat(e.target.value)}
        error={!isPasswordValid}
        InputProps={{
          endAdornment: (
            <InputAdornment position="end">
              <IconButton
                onClick={() => setShowPassword(!showPassword)}
                edge="end"
                aria-label="toggle password visibility"
              >
                {showPassword ? <VisibilityOff /> : <Visibility />}
              </IconButton>
            </InputAdornment>
          ),
        }}
      />
    </>
  );
}
