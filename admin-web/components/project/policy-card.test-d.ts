import type { ComponentProps } from "react";
import { PolicyCard } from "@/components/project/policy-card";
const props: ComponentProps<typeof PolicyCard> = { title:"Login policy", description:"x", available:true, rows:[{label:"Force MFA", value:"no"}] }; void props;
const degraded: ComponentProps<typeof PolicyCard> = { title:"Lockout policy", description:"", available:false, rows:[] }; void degraded;
