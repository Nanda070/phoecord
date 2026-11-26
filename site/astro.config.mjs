import { defineConfig } from "astro/config";

// Static site for https://phoecord.dev, deployed to GitHub Pages by .github/workflows/pages.yml.
export default defineConfig({
  site: "https://phoecord.dev",
  trailingSlash: "ignore",
  build: { format: "directory" },
});
