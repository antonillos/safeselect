import { Document, documentMetadata } from "../document";
export const metadata = documentMetadata("/read-only-is-not-a-boolean/");
export default function Article() {
  return <Document route="/read-only-is-not-a-boolean/" />;
}
