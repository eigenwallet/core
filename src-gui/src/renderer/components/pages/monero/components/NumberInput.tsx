import { useEffect, useRef, useState } from "react";
import { darken, useTheme } from "@mui/material";

interface NumberInputProps {
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  fontSize?: string;
  fontWeight?: number;
  textAlign?: "left" | "center" | "right";
  minWidth?: number;
  step?: number;
  largeStep?: number;
  className?: string;
  style?: React.CSSProperties;
}

export default function NumberInput({
  value,
  onChange,
  placeholder = "0.00",
  fontSize = "2em",
  fontWeight = 600,
  textAlign = "center",
  minWidth = 60,
  step = 0.001,
  largeStep = 0.1,
  className,
  style,
}: NumberInputProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [inputWidth, setInputWidth] = useState(minWidth);
  const measureRef = useRef<HTMLSpanElement>(null);

  const theme = useTheme();

  // Convert integer value to display value (divide by 1000)
  const displayValue = value
    ? (parseInt(value) / 1000).toFixed(3)
    : placeholder;

  // Convert placeholder to integer equivalent
  const placeholderAsInteger = (parseFloat(placeholder) * 1000).toString();

  // Initialize value to placeholder if empty
  useEffect(() => {
    if (!value) {
      onChange(placeholderAsInteger);
    }
  }, [placeholder, placeholderAsInteger, value, onChange]);

  // Measure text width to size input dynamically
  useEffect(() => {
    if (measureRef.current) {
      const text = displayValue;
      measureRef.current.textContent = text;
      const textWidth = measureRef.current.offsetWidth;
      setInputWidth(Math.max(textWidth + 5, minWidth)); // Add padding and minimum width
    }
  }, [displayValue, minWidth]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      const currentValue = parseInt(value) || 0;
      const increment = e.shiftKey ? largeStep * 1000 : step * 1000;
      const newValue =
        e.key === "ArrowUp"
          ? currentValue + increment
          : Math.max(0, currentValue - increment);
      onChange(newValue.toString());
    }
  };

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const inputValue = e.target.value;
    // Allow empty string, numbers, and decimal points
    if (inputValue === "" || /^\d*\.?\d*$/.test(inputValue)) {
      if (inputValue === "") {
        onChange("");
      } else {
        // Convert display value to integer (multiply by 1000)
        const integerValue = Math.round(parseFloat(inputValue) * 1000);
        onChange(integerValue.toString());
      }
    }
  };

  const handleBlur = () => {
    // If input is empty, set it back to placeholder
    if (!value || value.trim() === "") {
      onChange(placeholderAsInteger);
    }
    // No need to format here since we're storing as integer
  };

  const handleFocus = () => {
    // If the current value is the placeholder, clear it for editing
    if (value === placeholderAsInteger) {
      onChange("");
    }
  };

  // Determine if we should show placeholder styling
  const isPlaceholderValue = value === placeholderAsInteger;

  const defaultStyle: React.CSSProperties = {
    fontSize,
    fontWeight,
    textAlign,
    border: "none",
    outline: "none",
    background: "transparent",
    width: `${inputWidth}px`,
    minWidth: `${minWidth}px`,
    fontFamily: "inherit",
    color: isPlaceholderValue
      ? darken(theme.palette.text.primary, 0.5)
      : theme.palette.text.primary,
    padding: "4px 0",
    transition: "width 0.2s ease, color 0.2s ease",
    ...style,
  };

  return (
    <div style={{ position: "relative", display: "inline-block" }}>
      {/* Hidden span for measuring text width */}
      <span
        ref={measureRef}
        style={{
          position: "absolute",
          visibility: "hidden",
          fontSize,
          fontWeight,
          fontFamily: "inherit",
          whiteSpace: "nowrap",
        }}
      />

      <input
        ref={inputRef}
        type="text"
        inputMode="decimal"
        value={displayValue}
        onChange={handleChange}
        onKeyDown={handleKeyDown}
        onBlur={handleBlur}
        onFocus={handleFocus}
        className={className}
        style={defaultStyle}
      />
    </div>
  );
}
