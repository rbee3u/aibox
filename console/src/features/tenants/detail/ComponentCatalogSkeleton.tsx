import {
  HOST_COMPONENT_GROUPS,
  MANAGED_COMPONENT_GROUPS,
} from "@/features/tenants/componentCatalog";
import styles from "@/features/tenants/TenantPage.module.css";

/** Placeholder that keeps the catalog's group and card rhythm while loading. */
export function ComponentCatalogSkeleton({ host }: { host: boolean }) {
  const groups = host ? HOST_COMPONENT_GROUPS : MANAGED_COMPONENT_GROUPS;
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
          <div className={styles.componentGrid}>
            {group.kinds.map((kind) => (
              <div className={`${styles.componentCard} ${styles.componentSkeletonCard}`} key={kind}>
                <div className={styles.componentCardHeader}>
                  <span className={styles.skeletonIcon} />
                  <div className={styles.componentContent}>
                    <div className={styles.componentIdentity}>
                      <span className={styles.skeletonLine} />
                    </div>
                  </div>
                </div>
                <div className={styles.componentCardFooter}>
                  <span className={styles.componentSkeletonAction} />
                </div>
              </div>
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}
