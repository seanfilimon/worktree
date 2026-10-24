/**
 * Motion Configuration for 120fps Animations
 *
 * This configuration ensures smooth, high-performance animations
 * across the entire application using the motion library.
 */

import type { Transition, Variants } from "motion/react";

/**
 * Default transition settings optimized for 120fps
 */
export const defaultTransition: Transition = {
  type: "spring",
  stiffness: 260,
  damping: 20,
  mass: 1,
};

/**
 * Smooth easing transition for 120fps
 */
export const smoothTransition: Transition = {
  duration: 0.4,
  ease: [0.25, 0.1, 0.25, 1],
};

/**
 * Fast transition for quick interactions
 */
export const fastTransition: Transition = {
  duration: 0.2,
  ease: "easeOut",
};

/**
 * Slow transition for emphasis
 */
export const slowTransition: Transition = {
  duration: 0.8,
  ease: [0.4, 0, 0.2, 1],
};

/**
 * Spring animation configurations
 */
export const springs = {
  gentle: {
    type: "spring" as const,
    stiffness: 100,
    damping: 15,
  },
  bouncy: {
    type: "spring" as const,
    stiffness: 400,
    damping: 10,
  },
  stiff: {
    type: "spring" as const,
    stiffness: 500,
    damping: 30,
  },
};

/**
 * Common animation variants
 */
export const fadeInUp: Variants = {
  hidden: {
    opacity: 0,
    y: 20,
  },
  visible: {
    opacity: 1,
    y: 0,
    transition: smoothTransition,
  },
};

export const fadeInDown: Variants = {
  hidden: {
    opacity: 0,
    y: -20,
  },
  visible: {
    opacity: 1,
    y: 0,
    transition: smoothTransition,
  },
};

export const fadeInLeft: Variants = {
  hidden: {
    opacity: 0,
    x: -20,
  },
  visible: {
    opacity: 1,
    x: 0,
    transition: smoothTransition,
  },
};

export const fadeInRight: Variants = {
  hidden: {
    opacity: 0,
    x: 20,
  },
  visible: {
    opacity: 1,
    x: 0,
    transition: smoothTransition,
  },
};

export const scaleIn: Variants = {
  hidden: {
    opacity: 0,
    scale: 0.9,
  },
  visible: {
    opacity: 1,
    scale: 1,
    transition: smoothTransition,
  },
};

export const slideIn: Variants = {
  hidden: {
    opacity: 0,
    x: -100,
  },
  visible: {
    opacity: 1,
    x: 0,
    transition: {
      ...smoothTransition,
      duration: 0.5,
    },
  },
};

/**
 * Stagger animation for lists
 */
export const staggerContainer: Variants = {
  hidden: { opacity: 0 },
  visible: {
    opacity: 1,
    transition: {
      staggerChildren: 0.1,
      delayChildren: 0.2,
    },
  },
};

export const staggerItem: Variants = {
  hidden: { opacity: 0, y: 20 },
  visible: {
    opacity: 1,
    y: 0,
    transition: smoothTransition,
  },
};

/**
 * Hover and tap animations
 */
export const hoverScale = {
  scale: 1.05,
  transition: fastTransition,
};

export const tapScale = {
  scale: 0.95,
  transition: fastTransition,
};

export const hoverLift = {
  y: -5,
  scale: 1.02,
  transition: fastTransition,
};

/**
 * Viewport animation settings
 */
export const viewportOptions = {
  once: true,
  margin: "0px 0px -100px 0px",
  amount: 0.3 as const,
};

/**
 * Card hover animation
 */
export const cardHover = {
  scale: 1.03,
  y: -5,
  boxShadow: "0 10px 30px -15px rgba(0, 0, 0, 0.2)",
  transition: fastTransition,
};

/**
 * Button hover animation
 */
export const buttonHover = {
  scale: 1.05,
  transition: {
    duration: 0.2,
    ease: "easeOut",
  },
};

/**
 * Rotate animation
 */
export const rotate: Variants = {
  hidden: {
    opacity: 0,
    rotate: -10,
  },
  visible: {
    opacity: 1,
    rotate: 0,
    transition: smoothTransition,
  },
};

/**
 * Blur in animation
 */
export const blurIn: Variants = {
  hidden: {
    opacity: 0,
    filter: "blur(10px)",
  },
  visible: {
    opacity: 1,
    filter: "blur(0px)",
    transition: smoothTransition,
  },
};

/**
 * Page transition
 */
export const pageTransition: Variants = {
  hidden: {
    opacity: 0,
    y: 20,
  },
  visible: {
    opacity: 1,
    y: 0,
    transition: {
      duration: 0.5,
      ease: [0.25, 0.1, 0.25, 1],
    },
  },
  exit: {
    opacity: 0,
    y: -20,
    transition: {
      duration: 0.3,
      ease: [0.25, 0.1, 0.25, 1],
    },
  },
};

/**
 * Modal animation
 */
export const modalBackdrop: Variants = {
  hidden: {
    opacity: 0,
  },
  visible: {
    opacity: 1,
    transition: {
      duration: 0.2,
    },
  },
  exit: {
    opacity: 0,
    transition: {
      duration: 0.2,
      delay: 0.1,
    },
  },
};

export const modalContent: Variants = {
  hidden: {
    opacity: 0,
    scale: 0.95,
    y: 20,
  },
  visible: {
    opacity: 1,
    scale: 1,
    y: 0,
    transition: {
      duration: 0.3,
      ease: [0.25, 0.1, 0.25, 1],
    },
  },
  exit: {
    opacity: 0,
    scale: 0.95,
    y: 20,
    transition: {
      duration: 0.2,
    },
  },
};

/**
 * Loading animation
 */
export const pulse: Variants = {
  initial: {
    opacity: 0.6,
    scale: 1,
  },
  animate: {
    opacity: 1,
    scale: 1.05,
    transition: {
      duration: 0.8,
      repeat: Infinity,
      repeatType: "reverse",
    },
  },
};

/**
 * Bounce animation
 */
export const bounce = {
  y: [0, -10, 0],
  transition: {
    duration: 0.6,
    repeat: Infinity,
    ease: "easeInOut",
  },
};

/**
 * Shimmer loading animation
 */
export const shimmer = {
  backgroundPosition: ["200% 0", "-200% 0"],
  transition: {
    duration: 2,
    repeat: Infinity,
    ease: "linear",
  },
};

/**
 * Number counter animation helper
 */
export const counterTransition: Transition = {
  duration: 1.5,
  ease: "easeOut",
};
