const esbuild = require("esbuild");
const path = require("path");

const production = process.argv.includes("--production");
const watch = process.argv.includes("--watch");

async function main() {
  const ctx = await esbuild.context({
    entryPoints: [path.resolve(__dirname, "../src/ladder/webview/main.tsx")],
    bundle: true,
    outfile: path.resolve(__dirname, "../media/ladderWebview.js"),
    platform: "browser",
    format: "esm",
    sourcemap: !production,
    minify: production,
    loader: {
      ".tsx": "tsx",
      ".ts": "ts",
      ".css": "css",
    },
    define: {
      "process.env.NODE_ENV": production ? '"production"' : '"development"',
    },
    external: [],
    logLevel: "info",
  });

  if (watch) {
    await ctx.watch();
    console.log("Watching for changes...");
  } else {
    await ctx.rebuild();
    await ctx.dispose();
    console.log("Build complete!");
  }
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
