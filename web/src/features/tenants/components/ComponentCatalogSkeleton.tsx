import styles from "@/features/tenants/components/ComponentCatalogSkeleton.module.css";
import componentsStyles from "@/features/tenants/components/components.module.css";
import {
  HOST_COMPONENT_GROUPS,
  MANAGED_COMPONENT_GROUPS,
} from "@/features/tenants/components/componentCatalog";

/** Placeholder that keeps the catalog's group and card rhythm while loading. */
export function ComponentCatalogSkeleton({ host }: { host: boolean }) {
  const groups = host ? HOST_COMPONENT_GROUPS : MANAGED_COMPONENT_GROUPS;
  return (
    <div
      className={componentsStyles.componentCatalogSkeleton}
      role="progressbar"
      aria-label="Loading Components"
    >
      {groups.map((group) => (
        <section className={componentsStyles.componentGroup} key={group.id} aria-hidden="true">
          <div className={componentsStyles.componentGroupHeader}>
            <span className={styles.skeletonLine} />
          </div>
          <div className={componentsStyles.componentGrid}>
            {group.kinds.map((kind) => (
              <div
                className={`${componentsStyles.componentCard} ${styles.componentSkeletonCard}`}
                key={kind}
              >
                <div className={componentsStyles.componentCardHeader}>
                  <span className={styles.skeletonIcon} />
                  <div className={styles.componentContent}>
                    <div className={componentsStyles.componentIdentity}>
                      <span className={styles.skeletonLine} />
                    </div>
                  </div>
                </div>
                <div className={componentsStyles.componentCardFooter}>
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
