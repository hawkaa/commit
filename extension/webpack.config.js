const path = require("path");
const CopyWebpackPlugin = require("copy-webpack-plugin");
const MiniCssExtractPlugin = require("mini-css-extract-plugin");

module.exports = (env, argv) => {
  const isDev = argv.mode === "development";

  return {
    entry: {
      background: path.join(__dirname, "src", "background.ts"),
      "content-github": path.join(__dirname, "src", "content-github.ts"),
      "content-google": path.join(__dirname, "src", "content-google.ts"),
      // offscreen is NOT a webpack entry — it loads the pre-built UMD bundle
      // via <script> tag because the WASM worker chain has internal import
      // paths that webpack cannot safely rewrite.
    },

    output: {
      filename: "[name].js",
      path: path.resolve(__dirname, "build"),
      clean: true,
    },

    resolve: {
      extensions: [".ts", ".js"],
    },

    module: {
      rules: [
        {
          test: /\.ts$/,
          exclude: /node_modules/,
          use: {
            loader: "ts-loader",
            options: { transpileOnly: isDev },
          },
        },
        {
          test: /\.css$/,
          use: [MiniCssExtractPlugin.loader, "css-loader"],
        },
      ],
    },

    plugins: [
      new MiniCssExtractPlugin({ filename: "[name].css" }),

      new CopyWebpackPlugin({
        patterns: [
          { from: "src/manifest.json", to: "manifest.json" },
          { from: "icons", to: "icons" },
          { from: "src/offscreen.html", to: "offscreen.html" },
          { from: "src/trust-card.css", to: "trust-card.css" },
          // Offscreen JS — compiled separately by tsc, not webpack
          { from: "src/offscreen-bundle.js", to: "offscreen.js" },
          // Pre-built tlsn-js UMD bundle + all assets it references.
          // lib.js is a webpack UMD bundle that expects hashed filenames.
          { from: "node_modules/tlsn-js/build/lib.js", to: "tlsn-lib.js" },
          // Hashed WASM binary (referenced by lib.js as n.p+"96d03...wasm")
          { from: "node_modules/tlsn-js/build/96d038089797746d7695.wasm", to: "96d038089797746d7695.wasm" },
          // Hashed spawn worker (referenced by lib.js as n.p+"a6de6...js")
          { from: "node_modules/tlsn-js/build/a6de6b189c13ad309102.js", to: "a6de6b189c13ad309102.js" },
          // Raw wasm-bindgen files (referenced by spawn worker via import("../../../tlsn_wasm.js"))
          { from: "node_modules/tlsn-wasm/tlsn_wasm_bg.wasm", to: "tlsn_wasm_bg.wasm" },
          { from: "node_modules/tlsn-wasm/tlsn_wasm.js", to: "tlsn_wasm.js" },
          { from: "node_modules/tlsn-wasm/snippets", to: "snippets" },
        ],
      }),
    ],

    devtool: isDev ? "cheap-module-source-map" : false,
    optimization: { minimize: !isDev },
  };
};
