"use client";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { KeyRound, Lock, LogIn } from "lucide-react";
import { PageHeader } from "@/components/shell/PageHeader";
import { OrgCard } from "@/components/project/org-card";
import { ProjectCard } from "@/components/project/project-card";
import { PolicyCard, type PolicyRow } from "@/components/project/policy-card";
import { api, ApiError } from "@/lib/api";
import type {
  Org,
  Project,
  PolicyEnvelope,
  LoginPolicy,
  PasswordComplexityPolicy,
  LockoutPolicy,
} from "@/lib/types";

// A best-effort policy read degrades to "unavailable" on its OWN failure so one
// unreadable policy never blanks the others (design §9/§12). The runtime SA may
// lack the privilege to read a given org policy — that card shows the
// provisioner-managed note instead of erroring the whole page.
const UNAVAILABLE = { available: false, policy: null } as const;

// Protobuf Duration/count fields serialize as STRINGS ("0s","240h0m0s","8") and
// are shown verbatim; em-dash when the field is absent (design §9).
function str(v: string | undefined): string {
  return v ?? "—";
}
function yesNo(v: boolean | undefined): string {
  return v ? "yes" : "no";
}

export default function SettingsPage() {
  const [org, setOrg] = useState<Org | null>(null);
  const [project, setProject] = useState<Project | null>(null);
  const [login, setLogin] = useState<PolicyEnvelope<LoginPolicy>>(UNAVAILABLE);
  const [complexity, setComplexity] =
    useState<PolicyEnvelope<PasswordComplexityPolicy>>(UNAVAILABLE);
  const [lockout, setLockout] = useState<PolicyEnvelope<LockoutPolicy>>(UNAVAILABLE);

  const load = useCallback(async () => {
    // Best-effort org read: on failure the card shows "—" (org name is informational).
    try {
      setOrg(await api.get<Org>("/api/org"));
    } catch {
      setOrg(null);
    }
    try {
      setProject(await api.get<Project>("/api/project"));
    } catch (e) {
      if (!(e instanceof ApiError && e.status === 401)) {
        toast.error("Failed to load project");
      }
    }
    // Each policy read is independent: its own try/catch degrades to
    // {available:false,policy:null} so a single failure never blanks the rest.
    try {
      setLogin(await api.get<PolicyEnvelope<LoginPolicy>>("/api/org/policies/login"));
    } catch {
      setLogin(UNAVAILABLE);
    }
    try {
      setComplexity(
        await api.get<PolicyEnvelope<PasswordComplexityPolicy>>(
          "/api/org/policies/password-complexity",
        ),
      );
    } catch {
      setComplexity(UNAVAILABLE);
    }
    try {
      setLockout(await api.get<PolicyEnvelope<LockoutPolicy>>("/api/org/policies/lockout"));
    } catch {
      setLockout(UNAVAILABLE);
    }
  }, []);

  useEffect(() => {
    api.get("/api/me").catch(() => {});
    load();
  }, [load]);

  const loginRows: PolicyRow[] = [
    { label: "Username + password", value: yesNo(login.policy?.allowUsernamePassword) },
    { label: "Self-registration", value: yesNo(login.policy?.allowRegister) },
    { label: "External IdP", value: yesNo(login.policy?.allowExternalIdp) },
    { label: "Force MFA", value: yesNo(login.policy?.forceMfa) },
    { label: "Passwordless", value: str(login.policy?.passwordlessType) },
    { label: "MFA-init skip lifetime", value: str(login.policy?.mfaInitSkipLifetime) },
  ];
  const complexityRows: PolicyRow[] = [
    { label: "Min length", value: str(complexity.policy?.minLength) },
    { label: "Uppercase", value: yesNo(complexity.policy?.hasUppercase) },
    { label: "Lowercase", value: yesNo(complexity.policy?.hasLowercase) },
    { label: "Number", value: yesNo(complexity.policy?.hasNumber) },
    { label: "Symbol", value: yesNo(complexity.policy?.hasSymbol) },
  ];
  const lockoutRows: PolicyRow[] = [
    { label: "Max password attempts", value: str(lockout.policy?.maxPasswordAttempts) },
  ];

  return (
    <div className="space-y-4 px-6 py-6">
      <PageHeader
        title="Project & Org"
        description="The platform project is editable here. Org policies are read-only and provisioner-managed — changes are made out-of-band, not in the Console."
      />
      <OrgCard org={org} onSaved={load} />
      <ProjectCard project={project} onSaved={load} />
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
        <PolicyCard
          title="Login policy"
          description="How users authenticate into the platform org."
          available={login.available}
          rows={loginRows}
          icon={LogIn}
          iconClass="bg-blue-500/10 text-blue-600"
        />
        <PolicyCard
          title="Password complexity"
          description="Strength requirements enforced on org passwords."
          available={complexity.available}
          rows={complexityRows}
          icon={KeyRound}
          iconClass="bg-violet-500/10 text-violet-600"
        />
        <PolicyCard
          title="Lockout policy"
          description="Failed-attempt thresholds before an account locks."
          available={lockout.available}
          rows={lockoutRows}
          icon={Lock}
          iconClass="bg-amber-500/10 text-amber-700"
        />
      </div>
    </div>
  );
}
