/** Fixes for guesses models make about the basic pack's modules (see `Pack.hints`). */
export default {
  'limbs.hand': 'a hand goes in "foot", whatever the limb: "foot": "hand.grasp" (or "hand.pincer")',
  'limbs.wings': 'wings are limbs of their own: { "id": "wing", "role": "wing" }',
  'skin.material:feathers':
    'only wings have feathers: a wing limb with "membrane": "membrane.feather"',
  'limbs.role:pincer': 'pincers are hands: an arm with "foot": "hand.pincer"',
  'limbs.role:claw': 'claws are feet or hands: a leg or arm with "foot": "foot.claw"',
  'limbs.role:antenna': 'antennae are parts: { "id": "antennae", "type": "antenna" }',
  'limbs.role:mandible': 'mandibles are parts: { "id": "mandibles", "type": "mandible" }',
  'limbs.role:beak': 'a beak is a part: { "id": "beak", "type": "beak" }',
  'body.head.beak': 'a beak is a part: { "id": "beak", "type": "beak" }',
  'body.shell': 'a shell is a part: { "id": "shell", "type": "shell" }',
  'body.fins':
    'paired fins are limbs ({ "role": "fin" }); a dorsal fin is the part "fin.dorsal" and a tail fin "fin.tail"',
} as const;
