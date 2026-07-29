// GENERATED from docs/api/bindings/oauth2-jwt-issuer-next.json by
// scripts/gen-binding-types.mjs — do not edit by hand. Context: OAUTH2_SCRIPTED_JWT_ISSUER_NEXT_GEN.
// Shared next-gen-common bindings come from common.d.ts + nextgen-common.d.ts.

declare const issuer: StringLike;
interface IdRepository {
  createUser(userName: StringLike, password: StringLike): object;
  createUser(
    userName: StringLike,
    password: StringLike,
    attributes: object
  ): object;
  getIdentity(userName: StringLike): object;
}
declare const idRepository: IdRepository;

interface EmailService {
  send(to: StringLike, subject: StringLike, body: StringLike): void;
  send(
    to: StringLike,
    subject: StringLike,
    body: StringLike,
    mimeType: StringLike
  ): void;
}
declare const emailService: EmailService;
