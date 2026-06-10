import { redirect } from "next/navigation";

// The (dash) route group adds no URL segment, so this is the index for "/".
// The Console lands on the Dashboard (design §10).
export default function DashIndex() {
  redirect("/dashboard");
}
