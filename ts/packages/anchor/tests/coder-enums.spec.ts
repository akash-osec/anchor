import * as assert from "assert";
import { getStructCodec } from "@solana/kit";
import { getRustEnumCodec } from "../src/coder/borsh/codecs";

describe("borsh enum codec", () => {
  test("does not treat inherited constructor as an enum variant", () => {
    const codec = getRustEnumCodec([
      ["constructor", getStructCodec([])],
      ["withdraw", getStructCodec([])],
    ]);

    const encoded = codec.encode({ withdraw: {} });

    assert.deepStrictEqual([...encoded], [1]);
    assert.deepStrictEqual(codec.decode(encoded), { withdraw: {} });
  });

  test("does not treat other Object prototype names as enum variants", () => {
    const codec = getRustEnumCodec([
      ["toString", getStructCodec([])],
      ["hasOwnProperty", getStructCodec([])],
      ["withdraw", getStructCodec([])],
    ]);

    const encoded = codec.encode({ withdraw: {} });

    assert.deepStrictEqual([...encoded], [2]);
  });

  test("accepts enum values with a null prototype", () => {
    const codec = getRustEnumCodec([["withdraw", getStructCodec([])]]);
    const value = Object.create(null) as { withdraw: Record<string, never> };
    value.withdraw = {};

    assert.deepStrictEqual([...codec.encode(value)], [0]);
  });
});
