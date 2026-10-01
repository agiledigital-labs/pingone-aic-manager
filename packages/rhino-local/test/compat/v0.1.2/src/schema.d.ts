/** Contexts-endpoint binding metadata (`docs/api/bindings/*.json`). */
export declare const JS_TYPES: readonly ["string", "number", "boolean", "object", "array", "unknown", "void"];
export type JsType = (typeof JS_TYPES)[number];
export interface Parameter {
    name: string;
    javaScriptType: JsType;
}
export interface MethodElement {
    elementType: "method";
    name: string;
    parameters: Parameter[];
    returnType: JsType;
}
export interface FieldElement {
    elementType: "field";
    name: string;
    javaScriptType: JsType;
    elements: Element[];
}
export type Element = MethodElement | FieldElement;
export interface Binding {
    name: string;
    javaScriptType: JsType;
    elements: Element[];
}
export interface ContextsDocument {
    _id: string;
    bindings: Binding[];
}
export type BindingKind = "scalar" | "opaque" | "object";
export declare function classifyBinding(binding: Binding): BindingKind;
export interface MethodMember {
    kind: "method";
    name: string;
    overloads: MethodElement[];
}
export interface FieldMember {
    kind: "field";
    name: string;
    javaScriptType: JsType;
    elements: Element[];
}
export type Member = MethodMember | FieldMember;
/** Group overloads of the same name; preserve first-seen member order. */
export declare function membersOf(elements: Element[]): Member[];
export declare function uniqueOverloads(overloads: MethodElement[]): MethodElement[];
export declare function overloadKey(overload: MethodElement): string;
export declare function overloadLabel(overload: MethodElement): string;
