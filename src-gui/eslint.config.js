import globals from "globals";
import js from "@eslint/js";
import tseslint from "typescript-eslint";
import pluginReact from "eslint-plugin-react";
import importPlugin from "eslint-plugin-import";
import reactHooksPlugin from "eslint-plugin-react-hooks";

export default [
  { ignores: ["node_modules", "dist"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  pluginReact.configs.flat.recommended,
  reactHooksPlugin.configs.flat.recommended,
  {
    languageOptions: {
      globals: globals.browser,
    },
    plugins: {
      import: importPlugin,
    },
    settings: {
      react: {
        version: "detect",
      },
    },
    rules: {
      "react/react-in-jsx-scope": "off",
      "react/no-unescaped-entities": "off",
      "react/no-children-prop": "off",
      "@typescript-eslint/no-unused-vars": "off",
      "@typescript-eslint/no-empty-object-type": "off",
      // TODO: This is temporary until we fix all of our hook issues
      // We want to avoid eslint becoming useless for us by always failing
      "react-hooks/set-state-in-effect": "warn",
      "import/no-cycle": ["error", { maxDepth: 10 }],
      "no-restricted-globals": [
        "warn",
        {
          name: "open",
          message:
            "Use the openUrl(...) function from @tauri-apps/plugin-opener instead",
        },
      ],
      "no-restricted-properties": [
        "warn",
        {
          object: "window",
          property: "open",
          message:
            "Use the openUrl(...) function from @tauri-apps/plugin-opener instead",
        },
      ],
    },
  },
];
