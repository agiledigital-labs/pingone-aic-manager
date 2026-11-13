import { cleanCase, faultCase, PUT_UNDEFINED, SEND_DATE } from "./cases.ts";

faultCase("a date", SEND_DATE);
faultCase("a undefined", PUT_UNDEFINED);
faultCase("a declared", SEND_DATE, { declare: "script-defect/send-body-js-date" });
cleanCase("a clean");
