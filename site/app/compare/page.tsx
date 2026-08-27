import { Document, documentMetadata } from "../document";
export const metadata = documentMetadata("/compare/");
export default function Comparison() {
  return <Document route="/compare/" />;
}
