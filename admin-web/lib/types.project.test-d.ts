import type { Project, LoginPolicy, PasswordComplexityPolicy, LockoutPolicy, PolicyEnvelope } from "@/lib/types";
const p: Project = { id:"x", name:"llm-chat", projectRoleAssertion:true, projectRoleCheck:false, hasProjectCheck:true }; void p;
const login: PolicyEnvelope<LoginPolicy> = { available:true, policy:{ allowUsernamePassword:true, forceMfa:false, mfaInitSkipLifetime:"0s" } }; void login;
const pw: PasswordComplexityPolicy = { minLength:"8", hasUppercase:true, hasLowercase:true, hasNumber:true, hasSymbol:false }; void pw;
const lock: LockoutPolicy = { maxPasswordAttempts:"5" }; void lock;
const degraded: PolicyEnvelope<LockoutPolicy> = { available:false, policy:null }; void degraded;
