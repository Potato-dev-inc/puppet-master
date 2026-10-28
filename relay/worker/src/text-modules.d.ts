// The viewer files (relay/viewer/dist) are bundled as text by wrangler (`[[rules]] type = "Text"` in
// wrangler.toml), so each import is a string at run time.
declare module "*.html" {
  const text: string;
  export default text;
}

declare module "*.css" {
  const text: string;
  export default text;
}
