import {
  Box,
  ClickAwayListener,
  MenuItem,
  MenuList,
  Paper,
  Popper,
  TextField,
} from "@mui/material";
import { useEffect, useState } from "react";
import { getSeedWords } from "renderer/rpc";

const MAX_SUGGESTIONS = 6;

// Cached across mounts so the wordlist is only fetched once per session.
let cachedWords: string[] | null = null;

function currentWordOf(value: string): string {
  // The word being typed is the trailing token; a trailing space completes it.
  return value.split(/\s+/).pop() ?? "";
}

function suggestionsFor(value: string, words: string[]): string[] {
  const currentWord = currentWordOf(value);
  if (currentWord.length === 0) return [];

  return words
    .filter((word) => word.startsWith(currentWord) && word !== currentWord)
    .slice(0, MAX_SUGGESTIONS);
}

export default function SeedPhraseInput({
  value,
  onChange,
  error,
  helperText,
}: {
  value: string;
  onChange: (value: string) => void;
  error: boolean;
  helperText: string;
}) {
  const [words, setWords] = useState<string[]>(cachedWords ?? []);
  const [highlighted, setHighlighted] = useState(0);
  const [dismissed, setDismissed] = useState(false);
  const [anchorEl, setAnchorEl] = useState<HTMLDivElement | null>(null);

  useEffect(() => {
    if (cachedWords !== null) return;

    getSeedWords()
      .then((fetched) => {
        cachedWords = fetched;
        setWords(fetched);
      })
      .catch(() => setWords([]));
  }, []);

  const suggestions = suggestionsFor(value, words);
  const open = suggestions.length > 0 && !dismissed;

  const completeWith = (word: string) => {
    const trimmedEnd = value.replace(/\S*$/, "");
    onChange(`${trimmedEnd}${word} `);
  };

  const handleKeyDown = (event: React.KeyboardEvent) => {
    if (!open) return;

    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        setHighlighted((h) => (h + 1) % suggestions.length);
        break;
      case "ArrowUp":
        event.preventDefault();
        setHighlighted(
          (h) => (h - 1 + suggestions.length) % suggestions.length,
        );
        break;
      case "Enter":
      case "Tab":
        event.preventDefault();
        completeWith(suggestions[highlighted]);
        break;
      case "Escape":
        event.preventDefault();
        setDismissed(true);
        break;
    }
  };

  return (
    <Box ref={setAnchorEl} sx={{ position: "relative" }}>
      <TextField
        fullWidth
        multiline
        autoFocus
        rows={3}
        label="Enter your seed phrase"
        value={value}
        onChange={(e) => {
          setDismissed(false);
          setHighlighted(0);
          onChange(e.target.value);
        }}
        onKeyDown={handleKeyDown}
        placeholder="Enter your Monero 25 words seed phrase..."
        error={error}
        helperText={helperText}
      />
      <Popper
        open={open}
        anchorEl={anchorEl}
        placement="bottom-start"
        sx={{
          width: anchorEl?.clientWidth,
          zIndex: (theme) => theme.zIndex.modal + 1,
        }}
      >
        <ClickAwayListener onClickAway={() => setDismissed(true)}>
          <Paper elevation={4}>
            <MenuList dense disablePadding>
              {suggestions.map((word, index) => (
                <MenuItem
                  key={word}
                  selected={index === highlighted}
                  onMouseDown={(e) => {
                    // Keep focus in the textarea so typing continues.
                    e.preventDefault();
                    completeWith(word);
                  }}
                >
                  {word}
                </MenuItem>
              ))}
            </MenuList>
          </Paper>
        </ClickAwayListener>
      </Popper>
    </Box>
  );
}
