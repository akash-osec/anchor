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
});
