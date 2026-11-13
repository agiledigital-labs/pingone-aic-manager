import { cleanCase, faultCase, PUT_UNDEFINED } from "./cases.ts";

cleanCase("c clean");
faultCase("c undefined", PUT_UNDEFINED);
faultCase("c declared", PUT_UNDEFINED, { declare: "state-put-undefined" });
