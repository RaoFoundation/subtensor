import native from './native'
import { nativeCall } from './errors'
import { fromWire, toBigInt, toBuffer, toWire } from './wire'
import type {
  ByteLike,
  CoreValueDescriptor,
  CoreValueEntry,
  IntegerLike,
  ScaleValue,
} from './types'

function roundtrip(value: CoreValueDescriptor): CoreValueDescriptor {
  return nativeCall(
    () => native.coreValueDescriptorRoundtrip(value) as CoreValueDescriptor,
  )
}

function hex(value: ByteLike): `0x${string}` {
  return `0x${toBuffer(value, 'value').toString('hex')}`
}

export function coreValueDescriptorRoundtrip(
  value: CoreValueDescriptor,
): CoreValueDescriptor {
  return roundtrip(value)
}

/** Alias emphasizing that the descriptor is normalized by Rust. */
export const normalizeCoreValue = coreValueDescriptorRoundtrip

export function coreValueDescriptorToWire(value: CoreValueDescriptor): ScaleValue {
  return nativeCall(() => fromWire(native.coreValueDescriptorToWire(value)))
}

export function wireToCoreValueDescriptor(value: ScaleValue): CoreValueDescriptor {
  return nativeCall(
    () => native.wireToCoreValueDescriptor(toWire(value)) as CoreValueDescriptor,
  )
}

export function coreValueDescriptorToCorpusJson(
  value: CoreValueDescriptor,
): unknown {
  return nativeCall(() => native.coreValueDescriptorToCorpusJson(value))
}

export function coreValueDescriptorDisplay(value: CoreValueDescriptor): string {
  return nativeCall(() => native.coreValueDescriptorDisplay(value))
}

export function coreValueNull(): CoreValueDescriptor {
  return nativeCall(() => native.coreValueNull() as CoreValueDescriptor)
}

export function coreValueBool(value: boolean): CoreValueDescriptor {
  return nativeCall(() => native.coreValueBool(value) as CoreValueDescriptor)
}

export function coreValueInt(value: IntegerLike): CoreValueDescriptor {
  return nativeCall(
    () => native.coreValueInt(toBigInt(value, 'value')) as CoreValueDescriptor,
  )
}

export function coreValueUint(value: IntegerLike): CoreValueDescriptor {
  return nativeCall(
    () => native.coreValueUint(toBigInt(value, 'value')) as CoreValueDescriptor,
  )
}

export function coreValueU256Le(value: ByteLike): CoreValueDescriptor {
  return nativeCall(
    () => native.coreValueU256Le(toBuffer(value, 'value')) as CoreValueDescriptor,
  )
}

export function coreValueString(value: string): CoreValueDescriptor {
  return nativeCall(() => native.coreValueString(value) as CoreValueDescriptor)
}

export function coreValueBytes(value: ByteLike): CoreValueDescriptor {
  return nativeCall(
    () => native.coreValueBytes(toBuffer(value, 'value')) as CoreValueDescriptor,
  )
}

export function coreValueList(items: CoreValueDescriptor[]): CoreValueDescriptor {
  return nativeCall(() => native.coreValueList(items) as CoreValueDescriptor)
}

export function coreValueTuple(items: CoreValueDescriptor[]): CoreValueDescriptor {
  return nativeCall(() => native.coreValueTuple(items) as CoreValueDescriptor)
}

export function coreValueDict(entries: CoreValueEntry[]): CoreValueDescriptor {
  return nativeCall(() => native.coreValueDict(entries) as CoreValueDescriptor)
}

export function coreValueRecord(
  fields: Array<[name: string, value: CoreValueDescriptor]>,
): CoreValueDescriptor {
  return nativeCall(
    () =>
      native.coreValueRecord(
        fields.map(([name, value]) => ({ name, value })),
      ) as CoreValueDescriptor,
  )
}

/** Rust `Value::hex`: construct a string descriptor containing `0x` hex. */
export function coreValueHex(value: ByteLike): CoreValueDescriptor {
  return nativeCall(
    () => native.coreValueHex(toBuffer(value, 'value')) as CoreValueDescriptor,
  )
}

/** Descriptor-only constructors that do not cross FFI until normalized. */
export const coreValueDescriptor = Object.freeze({
  null(): CoreValueDescriptor {
    return { kind: 'null' }
  },
  bool(value: boolean): CoreValueDescriptor {
    return { kind: 'bool', value }
  },
  int(value: IntegerLike): CoreValueDescriptor {
    return { kind: 'int', value: toBigInt(value, 'value').toString() }
  },
  uint(value: IntegerLike): CoreValueDescriptor {
    return { kind: 'uint', value: toBigInt(value, 'value').toString() }
  },
  u256Le(value: ByteLike): CoreValueDescriptor {
    return { kind: 'u256', littleEndianHex: hex(value) }
  },
  str(value: string): CoreValueDescriptor {
    return { kind: 'str', value }
  },
  bytes(value: ByteLike): CoreValueDescriptor {
    return { kind: 'bytes', hex: hex(value) }
  },
  list(items: CoreValueDescriptor[]): CoreValueDescriptor {
    return { kind: 'list', items }
  },
  tuple(items: CoreValueDescriptor[]): CoreValueDescriptor {
    return { kind: 'tuple', items }
  },
  dict(entries: CoreValueEntry[]): CoreValueDescriptor {
    return { kind: 'dict', entries }
  },
})

export const coreValue = Object.freeze({
  null: coreValueNull,
  bool: coreValueBool,
  int: coreValueInt,
  uint: coreValueUint,
  u256Le: coreValueU256Le,
  str: coreValueString,
  bytes: coreValueBytes,
  list: coreValueList,
  tuple: coreValueTuple,
  dict: coreValueDict,
  record: coreValueRecord,
  hex: coreValueHex,
  roundtrip: coreValueDescriptorRoundtrip,
  normalize: normalizeCoreValue,
  toWire: coreValueDescriptorToWire,
  fromWire: wireToCoreValueDescriptor,
  toCorpusJson: coreValueDescriptorToCorpusJson,
  display: coreValueDescriptorDisplay,
})
