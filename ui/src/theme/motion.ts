/**
 * SwiftUI animation equivalents for Motion.
 *
 * `.spring(response:dampingFraction:)` is a unit-mass spring with
 *   stiffness = (2π / response)²
 *   damping   = 4π · dampingFraction / response
 */
import type { Transition } from "motion/react";

export function spring(response: number, dampingFraction: number): Transition {
  const stiffness = (2 * Math.PI / response) ** 2;
  const damping = (4 * Math.PI * dampingFraction) / response;
  return { type: "spring", stiffness, damping, mass: 1 };
}

/** `timingCurve(0.16, 1, 0.3, 1, duration:)` */
export const easeOutExpo = [0.16, 1, 0.3, 1] as const;

export const springs = {
  /** Tab bar selection (TabBarView). */
  tab: spring(0.32, 0.74),
  /** Tab label weight/scale change. */
  tabLabel: spring(0.28, 0.72),
  /** Press scale on bar buttons. */
  press: spring(0.22, 0.65),
  /** FLPillToggle active segment. */
  pill: spring(0.3, 0.78),
} as const;

export const durations = {
  tabSwitch: 0.25,
  queuePanel: 0.28,
  artworkZoom: 0.3,
  coverFade: 0.35,
  cardHover: 0.22,
  riseFadeIn: 0.28,
} as const;
