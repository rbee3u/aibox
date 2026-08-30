import { COMPONENT_GROUPS } from "@/features/tenants/componentCatalog";
import styles from "@/features/tenants/TenantPage.module.css";

/** Placeholder that keeps the catalog's group and row rhythm while loading. */
export function ComponentCatalogSkeleton({ host }: { host: boolean }) {
  const groups = host
    ? COMPONENT_GROUPS.filter((group) => group.id === "statuslines")
    : COMPONENT_GROUPS;
  return (
    <div
      className={styles.componentCatalogSkeleton}
      role="progressbar"
      aria-label="Loading Components"
    >
      {groups.map((group) => (
        <section className={styles.componentGroup} key={group.id} aria-hidden="true">
          <div className={styles.componentGroupHeader}>
            <span className={styles.skeletonLine} />
          </div>
          {group.kinds.map((kind) => (
            <div className={`${styles.componentRow} ${styles.componentSkeletonRow}`} key={kind}>
              <span className={styles.skeletonIcon} />
              <span className={styles.componentSkeletonContent}>
                <span className={styles.skeletonLine} />
                <span className={`${styles.skeletonLine} ${styles.skeletonLineShort}`} />
              </span>
              <span className={styles.componentSkeletonAction} />
            </div>
          ))}
        </section>
      ))}
    </div>
  );
}
