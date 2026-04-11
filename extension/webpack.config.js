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
      // prove-worker is a Web Worker entry point.
      // Webpack bundles it with all WASM/spawn worker paths resolved.
      // The offscreen document loads it via new Worker("prove-worker.js").
      "prove-worker": path.join(__dirname, "src", "prove-worker.ts"),
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
          // offscreen.js — plain JS relay (not webpack-bundled, no WASM deps)
          { from: "src/offscreen-bundle.js", to: "offscreen.js" },
          // WASM + worker assets referenced at runtime by the UMD bundle inside prove-worker.
          // These hashed filenames are hardcoded in tlsn-js/build/lib.js.
          { from: "node_modules/tlsn-js/build/96d038089797746d7695.wasm", to: "96d038089797746d7695.wasm" },
          { from: "node_modules/tlsn-js/build/a6de6b189c13ad309102.js", to: "a6de6b189c13ad309102.js" },
          // Raw wasm-bindgen files (referenced by spawn worker)
          { from: "node_modules/tlsn-wasm/tlsn_wasm_bg.wasm", to: "tlsn_wasm_bg.wasm" },
          { from: "node_modules/tlsn-wasm/tlsn_wasm.js", to: "tlsn_wasm.js" },
          { from: "node_modules/tlsn-wasm/snippets", to: "snippets" },
          // spawn.js is also requested at root level by sub-workers
          { from: "node_modules/tlsn-wasm/snippets/web-spawn-0303048270a97ee1/js/spawn.js", to: "spawn.js" },
        ],
      }),
    ],

    devtool: isDev ? "cheap-module-source-map" : false,
    optimization: { minimize: !isDev },
  };
};
