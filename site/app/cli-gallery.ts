import gallery from "../content/cli-gallery.json";

export type CliCommand = (typeof gallery.groups)[number]["commands"][number];
export type CliGroup = (typeof gallery.groups)[number];

export default gallery;
