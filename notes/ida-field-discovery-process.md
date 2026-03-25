# IDA Field Discovery Process for BProtoObject

## Function

- `BProtoObject::loadFromXml` @ `0x140341620` (size: 0x746c — massive)

## Pattern

Every field in this function follows the same structure:

```asm
lea     rdx, aFieldName     ; "FieldName"
mov     rcx, r15             ; String1
call    cs:__imp__stricmp
test    eax, eax
jnz     next_field
mov     rdx, [r14]           ; dereference this ptr (indirect)
add     rdx, XXh             ; struct offset
lea     rcx, [rsp+...]       ; xml reader context
call    BXMLReader__getTextAsFloat   ; or getTextAsInt
jmp     done
```

Some fields use `lea rdx, [r14+XXh]` (direct offset from r14) instead of `mov rdx, [r14]` + `add rdx, XXh` (indirect through pointer at [r14]).

## How to identify type

- `BXMLReader__getTextAsFloat` → `f32`
- `BXMLReader__getTextAsInt` → `i32`
- `BXMBData__getValueAsFloat` + `mulss` + `cvttss2si` → `Fract24` (store as `f32`)

## Steps

1. Pick an "extra field" warning from diagnostic output
2. Search the string in IDA (`find_regex`) → get address
3. Find xref to that string (`xrefs_to`) → locate in `loadFromXml`
4. Read disassembly: `add rdx, XXh` = struct offset, `getTextAsFloat/Int` = type
5. Comment in IDA (`set_comments`) as we go

## Confirmed Fields

| Field                 | Offset       | Type | Accessor       |
| --------------------- | ------------ | ---- | -------------- |
| FlattenMinX0          | \*this+0x20  | f32  | getTextAsFloat |
| FlattenMaxX0          | \*this+0x24  | f32  | getTextAsFloat |
| FlattenMinZ0          | \*this+0x28  | f32  | getTextAsFloat |
| FlattenMaxZ0          | \*this+0x2C  | f32  | getTextAsFloat |
| FlattenMinX1          | \*this+0x30  | f32  | getTextAsFloat |
| FlattenMaxX1          | \*this+0x34  | f32  | getTextAsFloat |
| ObstructionRadiusX    | \*this+0x44  | f32  | getTextAsFloat |
| ObstructionRadiusY    | \*this+0x48  | f32  | getTextAsFloat |
| ObstructionRadiusZ    | \*this+0x4C  | f32  | getTextAsFloat |
| AmmoMax               | this+0x88    | f32  | getTextAsFloat |
| AmmoRegenRate         | this+0x8C    | f32  | getTextAsFloat |
| NumConversions        | \*this+0x184 | i32  | getTextAsInt   |
| NumStasisFieldsToStop | \*this+0x188 | i32  | getTextAsInt   |
