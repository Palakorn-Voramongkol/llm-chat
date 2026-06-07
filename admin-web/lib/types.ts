export type UserKind = "Human" | "Machine";
export type UserState =
  | "ACTIVE" | "INACTIVE" | "LOCKED" | "INITIAL" | "DELETED" | "UNSPECIFIED";

export interface User {
  id: string;
  userName: string;
  kind: UserKind;
  state: UserState;
  email?: string;
  displayName?: string;
}

export interface UserList {
  result: User[];
  total?: number;
}

export interface Me {
  userId: string;
  name: string;
  roles: string[];
}

export interface Role {
  key: string;
  displayName: string;
  group?: string;
}

export interface UserGrant {
  grantId: string;
  projectId: string;
  roleKeys: string[];
}

export interface CreateHumanInput {
  userName: string;
  givenName: string;
  familyName: string;
  email: string;
  password?: string;
}

export interface CreateMachineInput {
  userName: string;
  name: string;
}

export interface EditProfileInput {
  givenName: string;
  familyName: string;
  displayName?: string;
}
