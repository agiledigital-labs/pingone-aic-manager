import { cleanCase, faultCase, PUT_UNDEFINED, SEND_DATE } from "./cases.ts";

faultCase("b both", `${SEND_DATE} } catch (e) {} try { ${PUT_UNDEFINED}`);
cleanCase("b clean");
faultCase("b date", SEND_DATE);
