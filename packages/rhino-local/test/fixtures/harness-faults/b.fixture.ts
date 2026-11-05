import { faultFile } from "./file.ts";

faultFile("b", { extra: 'nodeState.putShared("u", undefined);' });
